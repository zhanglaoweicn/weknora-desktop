Add-Type -AssemblyName System.IO.Compression.FileSystem
$src = 'D:\KnowLege\weknora-desktop\portable'
$dst = 'D:\KnowLege\weknora-desktop\LinkMindDesktop-Portable.zip'
if (Test-Path $dst) { Remove-Item $dst -Force }
$zip = [System.IO.Compression.ZipFile]::Open($dst, 'Create')
$count = 0
Get-ChildItem -Path $src -Recurse -File | Where-Object { $_.FullName -notmatch '\\data\\' } | ForEach-Object {
  $rel = 'portable/' + $_.FullName.Substring($src.Length + 1).Replace('\','/')
  [System.IO.Compression.ZipFileExtensions]::CreateEntryFromFile($zip, $_.FullName, $rel, 'Optimal') | Out-Null
  $count++
}
$zip.Dispose()
Write-Host ('files: ' + $count + ', size: ' + [math]::Round((Get-Item $dst).Length/1MB,1) + ' MB')
