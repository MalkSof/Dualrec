# Despliega Dualrec completo en AWS (o lo actualiza si ya existe).
#
#   powershell -ExecutionPolicy Bypass -File infra/aws/deploy.ps1 -AdminEmail vos@ejemplo.com
#
# Requisitos: AWS CLI v2 con credenciales (aws login / aws configure), Node.js.
# No hace falta Docker: la imagen de la API se construye en AWS (CodeBuild).
#
# Crea recursos que se cobran (RDS, Fargate, balanceador, S3, CloudFront).
# El costo aproximado esta en infra/aws/README.md.
param(
    [Parameter(Mandatory = $true)][string]$AdminEmail,
    [string]$AdminName = "Admin",
    [string]$Region = "us-east-1",
    [string]$BuildStack = "dualrec-build",
    [string]$AppStack = "dualrec",
    # Solo vuelve a publicar la web (no toca la API ni la infraestructura).
    [switch]$WebOnly
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$env:AWS_DEFAULT_REGION = $Region
$env:AWS_PAGER = ""

function Invoke-Aws {
    # Ejecuta la CLI y corta el script si falla.
    $output = & aws @args
    if ($LASTEXITCODE -ne 0) { throw "aws $($args -join ' ') fallo (codigo $LASTEXITCODE)" }
    return $output
}

function Get-StackOutput([string]$Stack, [string]$Key) {
    Invoke-Aws cloudformation describe-stacks --stack-name $Stack `
        --query "Stacks[0].Outputs[?OutputKey=='$Key'].OutputValue" --output text
}

$account = Invoke-Aws sts get-caller-identity --query Account --output text
Write-Output "Cuenta $account, region $Region"

if (-not $WebOnly) {
    # 1) Repositorio de imagenes y proyecto de construccion.
    Write-Output "`n[1/5] Infraestructura de construccion ($BuildStack)..."
    Invoke-Aws cloudformation deploy --stack-name $BuildStack `
        --template-file (Join-Path $PSScriptRoot "build.yaml") `
        --capabilities CAPABILITY_IAM --no-fail-on-empty-changeset | Out-Null
    $repository = Get-StackOutput $BuildStack "RepositoryUri"
    $sourceBucket = Get-StackOutput $BuildStack "SourceBucket"
    $project = Get-StackOutput $BuildStack "BuildProject"

    # 2) Se sube el codigo de la API y se construye la imagen en AWS.
    Write-Output "`n[2/5] Construyendo la imagen de la API en CodeBuild..."
    $tag = Get-Date -Format "yyyyMMdd-HHmmss"
    $staging = Join-Path ([System.IO.Path]::GetTempPath()) ("dualrec-api-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force $staging | Out-Null
    try {
        $api = Join-Path $root "apps\api"
        foreach ($item in "app", "alembic", "alembic.ini", "pyproject.toml", "uv.lock", "Dockerfile", "entrypoint.sh", ".dockerignore") {
            Copy-Item -Recurse -Force (Join-Path $api $item) $staging
        }
        Get-ChildItem $staging -Recurse -Directory -Filter "__pycache__" | Remove-Item -Recurse -Force
        $zip = Join-Path ([System.IO.Path]::GetTempPath()) "dualrec-api-$tag.zip"
        # tar escribe las rutas del zip con "/", que es lo que espera Linux.
        & tar -a -c -f $zip -C $staging .
        if ($LASTEXITCODE -ne 0) { throw "no se pudo comprimir el codigo de la API" }
        Invoke-Aws s3 cp $zip "s3://$sourceBucket/api.zip" --only-show-errors | Out-Null
        Remove-Item $zip -Force
    }
    finally {
        Remove-Item -Recurse -Force $staging -ErrorAction SilentlyContinue
    }

    $buildId = Invoke-Aws codebuild start-build --project-name $project `
        --environment-variables-override "name=IMAGE_TAG,value=$tag" --query "build.id" --output text
    do {
        Start-Sleep -Seconds 10
        $status = Invoke-Aws codebuild batch-get-builds --ids $buildId --query "builds[0].buildStatus" --output text
        Write-Output "   build: $status"
    } while ($status -eq "IN_PROGRESS")
    if ($status -ne "SUCCEEDED") {
        throw "La construccion de la imagen fallo ($status). Ver los logs de CodeBuild del proyecto $project."
    }

    # 3) El balanceador solo acepta trafico de CloudFront.
    $prefixList = Invoke-Aws ec2 describe-managed-prefix-lists `
        --filters "Name=prefix-list-name,Values=com.amazonaws.global.cloudfront.origin-facing" `
        --query "PrefixLists[0].PrefixListId" --output text

    # 4) La aplicacion: red, S3, RDS, ECS y CloudFront. La primera vez tarda 15-20 minutos.
    Write-Output "`n[3/5] Desplegando la aplicacion ($AppStack). La primera vez tarda 15-20 minutos..."
    Invoke-Aws cloudformation deploy --stack-name $AppStack `
        --template-file (Join-Path $PSScriptRoot "app.yaml") `
        --capabilities CAPABILITY_IAM --no-fail-on-empty-changeset `
        --parameter-overrides "ImageUri=${repository}:$tag" "CloudFrontPrefixListId=$prefixList" `
        "AdminEmail=$AdminEmail" "AdminName=$AdminName" | Out-Null
}

# 5) La web: se compila apuntando a /api (mismo dominio) y se publica en S3.
Write-Output "`n[4/5] Publicando la web..."
$webBucket = Get-StackOutput $AppStack "WebBucket"
$distribution = Get-StackOutput $AppStack "DistributionId"
$url = Get-StackOutput $AppStack "Url"
$apiUrl = Get-StackOutput $AppStack "ApiUrl"
$secretArn = Get-StackOutput $AppStack "AdminTokenSecretArn"

Push-Location (Join-Path $root "apps\web")
try {
    $env:VITE_API_URL = "/api"
    & npm ci --silent
    if ($LASTEXITCODE -ne 0) { throw "npm ci fallo" }
    & npm run build --silent
    if ($LASTEXITCODE -ne 0) { throw "la compilacion de la web fallo" }
    # Los archivos con hash se guardan un anio; index.html nunca, para que cada publicacion se vea al instante.
    Invoke-Aws s3 sync dist "s3://$webBucket" --delete --only-show-errors `
        --exclude index.html --cache-control "public,max-age=31536000,immutable" | Out-Null
    Invoke-Aws s3 cp dist/index.html "s3://$webBucket/index.html" --only-show-errors `
        --cache-control "no-cache" --content-type "text/html; charset=utf-8" | Out-Null
}
finally {
    Pop-Location
    Remove-Item Env:\VITE_API_URL -ErrorAction SilentlyContinue
}
Invoke-Aws cloudfront create-invalidation --distribution-id $distribution --paths "/*" | Out-Null

Write-Output "`n[5/5] Listo."
Write-Output ""
Write-Output "  Web (biblioteca y links):   $url"
Write-Output "  Direccion para la app:      $apiUrl"
Write-Output ""
Write-Output "  Token de acceso del usuario $AdminEmail (va en la app, Ajustes - Nube, y en la web):"
Write-Output "    aws secretsmanager get-secret-value --secret-id $secretArn --query SecretString --output text --region $Region"
