$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
  # Windows PowerShell 5 turns redirected native stderr warnings into error records.
  # The native exit code is authoritative; CMake/Cargo warnings are not failures.
  $nodeCommand = (Get-Command node -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
  $global:LASTEXITCODE = $null
  $ErrorActionPreference = 'Continue'
  & $nodeCommand scripts/build_windows.mjs build @args
  $buildExitCode = $global:LASTEXITCODE
  $ErrorActionPreference = 'Stop'
  if ($null -eq $buildExitCode) { throw 'Node could not start' }
  if ($buildExitCode -ne 0) { exit $buildExitCode }
}
catch {
  Write-Error -ErrorRecord $_ -ErrorAction Continue
  exit 1
}
finally { Pop-Location }
