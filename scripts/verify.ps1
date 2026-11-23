param(
    [string] $Cli = (Join-Path $PSScriptRoot '../target/release/list-rs.exe')
)

$ErrorActionPreference = 'Stop'
$executable = (Resolve-Path -LiteralPath $Cli).Path
$fixtures = Join-Path $PSScriptRoot '../fixtures'
$names = @(
    'Complex_numbers_variant_24.xmcd'
    'Complex_numbers_variant_24_calculations.xmcd'
    'Complex_numbers_variant_24_arctg_arcctg.xmcd'
)

foreach ($name in $names) {
    & $executable (Join-Path $fixtures $name) --check
    if ($LASTEXITCODE -ne 0) {
        throw "Ошибка сверки в $name (код $LASTEXITCODE)."
    }
}
