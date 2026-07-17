[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
  [string]$InstallerPath,

  [Parameter(Mandatory)]
  [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
  [string]$ChecksumsPath,

  [Parameter(Mandatory)]
  [string]$ExpectedPublisher
)

$installer = Get-Item -LiteralPath $InstallerPath
$checksum = Find-ExpectedChecksum -ChecksumsPath $ChecksumsPath -FileName $installer.Name
$actual = (Get-FileHash -LiteralPath $installer.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $checksum) { throw "SHA-256 mismatch for $($installer.Name)." }

$signature = Get-AuthenticodeSignature -LiteralPath $installer.FullName
if ($signature.Status -ne "Valid") { throw "Authenticode signature is not valid: $($signature.Status)." }
if ($signature.SignerCertificate.Subject -notlike "*$ExpectedPublisher*") {
  throw "Unexpected Authenticode publisher: $($signature.SignerCertificate.Subject)."
}

Write-Host "Verified SHA-256 and Authenticode publisher for $($installer.Name)."

function Find-ExpectedChecksum([string]$ChecksumsPath, [string]$FileName) {
  $match = Get-Content -LiteralPath $ChecksumsPath | Where-Object {
    $parts = $_ -split "\s+", 2
    $parts.Count -eq 2 -and [IO.Path]::GetFileName($parts[1].Trim().TrimStart("*")) -eq $FileName
  }
  if (@($match).Count -ne 1) { throw "Expected exactly one SHA-256 entry for $FileName." }
  return ($match -split "\s+", 2)[0].ToLowerInvariant()
}
