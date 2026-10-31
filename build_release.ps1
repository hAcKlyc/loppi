$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
  $ErrorActionPreference = 'Continue'
  node scripts/build_windows.mjs release
  $buildExitCode = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($buildExitCode -ne 0) { exit $buildExitCode }
}
finally { Pop-Location }
