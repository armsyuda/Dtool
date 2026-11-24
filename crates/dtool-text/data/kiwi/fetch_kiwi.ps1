param([string]$Dest = (Join-Path $PSScriptRoot '.'))
$ErrorActionPreference = 'Stop'
$base = 'https://github.com/bab2min/Kiwi/releases/download/v0.24.1'
$assets = @{
    'kiwi_win_x64_v0.24.1.zip'    = '857576C5E8492EC66F8327E35572B20F286B092A8E760FE780ACE1A99AFC3B30'
    'kiwi_model_v0.24.1_base.tgz' = '50D163E1AF817444EF21F7EC5EA07EF69FB347282D57A6B49260F2039D4DC346'
}
$dl = Join-Path $Dest 'dl'
New-Item -ItemType Directory -Force $dl | Out-Null
foreach ($name in $assets.Keys) {
    $file = Join-Path $dl $name
    if (-not (Test-Path $file)) { curl.exe -sSL -o $file "$base/$name" }
    $hash = (Get-FileHash $file -Algorithm SHA256).Hash
    if ($hash -ne $assets[$name]) { throw "sha256 mismatch for ${name}: $hash" }
}
$unzip = Join-Path $dl 'win'
Expand-Archive -Force (Join-Path $dl 'kiwi_win_x64_v0.24.1.zip') -DestinationPath $unzip
Copy-Item -Force (Join-Path $unzip 'lib\kiwi.dll') (Join-Path $Dest 'kiwi.dll')
$untar = Join-Path $dl 'model'
New-Item -ItemType Directory -Force $untar | Out-Null
tar -xzf (Join-Path $dl 'kiwi_model_v0.24.1_base.tgz') -C $untar
$model = Join-Path $Dest 'model'
New-Item -ItemType Directory -Force $model | Out-Null
Copy-Item -Force (Join-Path $untar 'models\cong\base\*') $model
$here = (Resolve-Path $PSScriptRoot).Path
$target = (Resolve-Path $Dest).Path
if ($target -ne $here) {
    foreach ($notice in 'LICENSE', 'NOTICE', 'THIRD_PARTY_NOTICES.md') {
        Copy-Item -Force (Join-Path $here $notice) (Join-Path $target $notice)
    }
}
Write-Output "kiwi bundle ready in $Dest"
