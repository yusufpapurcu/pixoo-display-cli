$ErrorActionPreference = 'Stop'
$projectDir = Split-Path $PSScriptRoot -Parent
$documents = @(Get-Item (Join-Path $projectDir 'README.md'),(Join-Path $projectDir 'NOTICE.md')) + @(Get-ChildItem (Join-Path $projectDir 'docs') -File -Filter '*.md')
$checked = 0
foreach ($document in $documents) {
    $matches = [regex]::Matches((Get-Content -LiteralPath $document.FullName -Raw),'\]\(([^)]+)\)')
    foreach ($match in $matches) {
        $target = ($match.Groups[1].Value -split '#')[0]
        if (!$target -or $target -match '^[a-zA-Z]+:') { continue }
        $path = Join-Path $document.DirectoryName $target
        if (!(Test-Path -LiteralPath $path)) { throw "Broken local link in $($document.Name): $target" }
        $checked++
    }
}
Write-Output "Verified $checked local documentation links."
