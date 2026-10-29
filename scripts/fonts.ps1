# Fetch the bundled fonts into crates\export\assets\fonts (the app's dev folder, also read by the
# HTML export). The binaries are not in git (about 51 MB); run this before running from the repo
# and before scripts\dist.ps1. Files already there with the right SHA-256 are skipped.
#   Noto Serif SC (SIL OFL 1.1)        body and headings, CJK
#   LXGW WenKai (SIL OFL 1.1)          blockquotes and CJK emphasis
#   Latin Modern Roman (GUST Font License)  Latin text, regular / bold / italic / bold italic
# Run: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\fonts.ps1
$ErrorActionPreference = 'Stop'
$dir = Join-Path $PSScriptRoot '..\crates\export\assets\fonts'
New-Item -ItemType Directory -Force $dir | Out-Null
$dir = (Resolve-Path $dir).Path

function Get-Sha256($path) { (Get-FileHash $path -Algorithm SHA256).Hash.ToLower() }

function Save-Url($url, $path) {
    # curl.exe follows the session proxy; Windows PowerShell 5.1 Invoke-WebRequest does not.
    $curl = Get-Command curl.exe -ErrorAction SilentlyContinue
    if ($curl) {
        & $curl.Source -sSL --fail -o $path $url
        if ($LASTEXITCODE) { throw "download failed: $url" }
    } else {
        Invoke-WebRequest $url -OutFile $path -UseBasicParsing
    }
}

$files = @(
    @{ Name = 'NotoSerifSC-VF.ttf'; Sha256 = '5326cfb097e3ab26fcb39329752b5c0a439bf8d5c4649520e4b492939c352a09'
       Url = 'https://github.com/notofonts/noto-cjk/raw/Serif2.003/Serif/Variable/TTF/Subset/NotoSerifSC-VF.ttf' },
    @{ Name = 'LXGWWenKai-Regular.ttf'; Sha256 = '2d73eea94ad808999d90c0000c239a84a33b09d4bc6ab6c5b8bbf1eda8a7fc8b'
       Url = 'https://github.com/lxgw/LxgwWenKai/releases/download/v1.530/LXGWWenKai-Regular.ttf' }
)
foreach ($f in $files) {
    $path = Join-Path $dir $f.Name
    if ((Test-Path $path) -and (Get-Sha256 $path) -eq $f.Sha256) { continue }
    Save-Url $f.Url $path
    $hash = Get-Sha256 $path
    if ($hash -ne $f.Sha256) { Remove-Item $path; throw "$($f.Name) SHA-256 mismatch: $hash" }
}

# Latin Modern 2.004 (OpenType) from GUST: one zip, four members kept.
$lm = @{
    'lmroman10-regular.otf'    = '1aa18cfefa58132c52ce5de70db1fd1154201c19cd2b2cdaffba4906a33e6852'
    'lmroman10-bold.otf'       = '102fe06c430a8b681b2bf6876b7cd967ae4d47b4b6b41d915eb7913b726d9fb1'
    'lmroman10-italic.otf'     = 'c1fce25075567bb8dbf2151658c3b442690041db17a2d49fc9e55905ea5b7169'
    'lmroman10-bolditalic.otf' = 'c37a28eed7a6e03f792b98b5e5f637b2fcda378bb4855f99284f1a88fe35f124'
}
$missing = $lm.Keys | Where-Object { -not (Test-Path (Join-Path $dir $_)) -or (Get-Sha256 (Join-Path $dir $_)) -ne $lm[$_] }
if ($missing) {
    $zipSha = '5b0236051d3728be6616f1b274e3b910473875b5a3ef9382f0ef00384ddb161b'
    $zip = Join-Path ([IO.Path]::GetTempPath()) "lm2.004otf-$([guid]::NewGuid().ToString('N')).zip"
    try {
        Save-Url 'https://www.gust.org.pl/projects/e-foundry/latin-modern/download/lm2.004otf.zip' $zip
        $hash = Get-Sha256 $zip
        if ($hash -ne $zipSha) { throw "lm2.004otf.zip SHA-256 mismatch: $hash" }
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $archive = [IO.Compression.ZipFile]::OpenRead($zip)
        try {
            foreach ($name in $missing) {
                $entry = $archive.Entries | Where-Object { $_.FullName -eq $name } | Select-Object -First 1
                if (-not $entry) { throw "$name not in the archive" }
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $dir $name), $true)
                $got = Get-Sha256 (Join-Path $dir $name)
                if ($got -ne $lm[$name]) { throw "$name SHA-256 mismatch: $got" }
            }
        } finally { $archive.Dispose() }
    } finally { Remove-Item $zip -ErrorAction SilentlyContinue }
}
Write-Output "fonts ready in $dir"
