param(
    [Parameter(Mandatory)][string]$AppExe,
    [Parameter(Mandatory)][string]$RuntimeDirectory,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [Parameter(Mandatory)][string]$VCRuntimeDll,
    [string]$MakeNsis = 'makensis.exe'
)

$ErrorActionPreference = 'Stop'
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
$releaseRoot = Split-Path -Parent $PSScriptRoot
$versionText = Get-Content (Join-Path $releaseRoot 'Cargo.toml') -Raw
if ($versionText -notmatch '(?m)^version = "(\d+\.\d+\.\d+)"$') {
    throw 'The workspace version must have three numeric components.'
}
$version = $Matches[1]
$payload = Join-Path $OutputDirectory 'payload'
if (Test-Path -LiteralPath $payload) { throw "Payload directory already exists at $payload" }
$null = New-Item -ItemType Directory -Path (Join-Path $payload 'docs')

Copy-Item -LiteralPath $AppExe -Destination (Join-Path $payload 'dtool.exe')
Copy-Item -LiteralPath $VCRuntimeDll -Destination (Join-Path $payload 'vcruntime140.dll')
foreach ($folder in 'kiwi', 'hunspell', 'licenses') {
    Copy-Item -LiteralPath (Join-Path $RuntimeDirectory $folder) -Destination $payload -Recurse
}

foreach ($guide in 'User-Guide', 'MCP-Guide') {
    $markdown = Get-Content -LiteralPath (Join-Path $releaseRoot "docs/$guide.md") -Raw
    $body = (ConvertFrom-Markdown -InputObject ($markdown.Replace('(MCP-Guide.md)', '(MCP-Guide.html)'))).Html
    $html = @"
<!doctype html>
<html lang="ko">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>DTool 0.1</title>
<style>body{font:16px/1.65 system-ui,sans-serif;max-width:50rem;margin:3rem auto;padding:0 1rem}pre{overflow:auto;background:#f1f1f1;padding:1rem}</style>
<main>
$body
</main>
</html>
"@
    Set-Content -LiteralPath (Join-Path $payload "docs/$guide.html") -Value $html -Encoding utf8
}

$uninstallFiles = Join-Path $OutputDirectory 'uninstall-files.nsh'
$files = Get-ChildItem -LiteralPath $payload -File -Recurse | Sort-Object FullName
$directories = Get-ChildItem -LiteralPath $payload -Directory -Recurse | Sort-Object { $_.FullName.Length } -Descending
$deleteCommands = foreach ($file in $files) {
    $relative = [IO.Path]::GetRelativePath($payload, $file.FullName).Replace('$', '$$')
    '  Delete "$INSTDIR\' + $relative + '"'
}
$removeCommands = foreach ($folder in $directories) {
    $relative = [IO.Path]::GetRelativePath($payload, $folder.FullName).Replace('$', '$$')
    '  RMDir "$INSTDIR\' + $relative + '"'
}
Set-Content -LiteralPath $uninstallFiles -Value ($deleteCommands + $removeCommands) -Encoding utf8BOM

$setup = Join-Path $OutputDirectory "Dtool-$version-Windows-x64-Setup.exe"
& $MakeNsis /V3 "/DDTOOL_VERSION=$version" "/DSETUP_EXE=$setup" "/DPAYLOAD=$payload" "/DUNINSTALL_FILES=$uninstallFiles" (Join-Path $PSScriptRoot 'Dtool.nsi')
if ($LASTEXITCODE -ne 0) { throw "NSIS failed with exit code $LASTEXITCODE" }
$digest = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant()
Set-Content -LiteralPath "$setup.sha256" -Value "$digest  $([IO.Path]::GetFileName($setup))" -Encoding ascii
Get-Item -LiteralPath $setup
