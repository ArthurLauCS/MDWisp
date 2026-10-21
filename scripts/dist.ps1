# Build the release exe and the installer: target\installer\MDWisp-<version>-setup.exe.
# Needs Inno Setup 6. Run: powershell -NoProfile -ExecutionPolicy Bypass -File scripts\dist.ps1
$root = "$PSScriptRoot\.."
$out = "$root\target\installer"
# Only this version's installer is kept; older ones live on GitHub Releases.
Remove-Item -Recurse -Force $out -ErrorAction SilentlyContinue
cargo build --release -p mdwisp --manifest-path "$root\Cargo.toml"
if ($LASTEXITCODE) { exit $LASTEXITCODE }
$version = (Select-String -Path "$root\crates\app\Cargo.toml" -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
# Inno Setup installs machine-wide or, from winget --scope user, per user.
$iscc = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1
& $iscc /Q "/DVersion=$version" "$root\installer\mdwisp.iss"
exit $LASTEXITCODE
