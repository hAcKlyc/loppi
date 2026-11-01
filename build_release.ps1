$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
  $nodeCommand = (Get-Command node -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
  $global:LASTEXITCODE = $null
  $ErrorActionPreference = 'Continue'
  & $nodeCommand scripts/build_windows.mjs release @args
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
