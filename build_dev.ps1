$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
  # Windows PowerShell 5 turns redirected native stderr warnings into error records.
  # The native exit code is authoritative; CMake/Cargo warnings are not failures.
  $ErrorActionPreference = 'Continue'
  node scripts/build_windows.mjs build
  $buildExitCode = $LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($buildExitCode -ne 0) { exit $buildExitCode }
}
finally { Pop-Location }
