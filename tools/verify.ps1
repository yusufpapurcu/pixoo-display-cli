param([switch]$Offline)
$ErrorActionPreference = 'Stop'
$projectDir = Split-Path $PSScriptRoot -Parent
$previous = @{ TMP=$env:TMP; TEMP=$env:TEMP; CARGO_INCREMENTAL=$env:CARGO_INCREMENTAL; RUSTDOCFLAGS=$env:RUSTDOCFLAGS }
Push-Location $projectDir
try {
    $testTemp = Join-Path $projectDir 'target/test-temp'
    New-Item -ItemType Directory -Force $testTemp | Out-Null
    $env:TMP = $testTemp
    $env:TEMP = $testTemp
    $env:CARGO_INCREMENTAL = '0'
    $env:RUSTDOCFLAGS = '-D warnings'
    & (Join-Path $PSScriptRoot 'update-coverage.ps1') -Check
    & (Join-Path $PSScriptRoot 'check-doc-links.ps1')
    $commands = @(
        @('fmt','--check'),
        @('test','--all-features'),
        @('test','--no-default-features','--all-targets'),
        @('clippy','--all-features','--all-targets'),
        @('doc','--all-features','--no-deps'),
        @('run','--example','media_native')
    )
    foreach ($cargoArgs in $commands) {
        if ($Offline -and $cargoArgs[0] -ne 'fmt') { $cargoArgs += '--offline' }
        if ($cargoArgs[0] -eq 'clippy') { $cargoArgs += @('--','-D','warnings') }
        & cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) { throw "cargo $($cargoArgs -join ' ') failed with exit $LASTEXITCODE" }
    }
} finally {
    $env:TMP = $previous.TMP
    $env:TEMP = $previous.TEMP
    $env:CARGO_INCREMENTAL = $previous.CARGO_INCREMENTAL
    $env:RUSTDOCFLAGS = $previous.RUSTDOCFLAGS
    Pop-Location
}
