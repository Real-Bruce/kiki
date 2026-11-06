param(
    [string]$Source = 'artifacts/p2/single/capture.png',
    [string]$Region = 'artifacts/p2/single/capture-region.png'
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$sourceBitmap = [System.Drawing.Bitmap]::new((Resolve-Path -LiteralPath $Source).Path)
$regionBitmap = $null
$sourceData = $null
$regionData = $null
try {
    $regionBitmap = [System.Drawing.Bitmap]::new((Resolve-Path -LiteralPath $Region).Path)
    $left = [int][math]::Floor($sourceBitmap.Width / 4)
    $top = [int][math]::Floor($sourceBitmap.Height / 4)
    $width = [int][math]::Floor($sourceBitmap.Width * 3 / 4) - $left
    $height = [int][math]::Floor($sourceBitmap.Height * 3 / 4) - $top
    if ($regionBitmap.Width -ne $width -or $regionBitmap.Height -ne $height) { throw '裁剪 PNG 尺寸不匹配' }
    $format = [System.Drawing.Imaging.PixelFormat]::Format32bppArgb
    $mode = [System.Drawing.Imaging.ImageLockMode]::ReadOnly
    $sourceData = $sourceBitmap.LockBits([System.Drawing.Rectangle]::new(0,0,$sourceBitmap.Width,$sourceBitmap.Height),$mode,$format)
    $regionData = $regionBitmap.LockBits([System.Drawing.Rectangle]::new(0,0,$width,$height),$mode,$format)
    $expected = [byte[]]::new($width * 4)
    $actual = [byte[]]::new($width * 4)
    for ($row=0; $row -lt $height; $row++) {
        [System.Runtime.InteropServices.Marshal]::Copy([IntPtr]::Add($sourceData.Scan0,($row+$top)*$sourceData.Stride+$left*4),$expected,0,$expected.Length)
        [System.Runtime.InteropServices.Marshal]::Copy([IntPtr]::Add($regionData.Scan0,$row*$regionData.Stride),$actual,0,$actual.Length)
        if (-not [System.Linq.Enumerable]::SequenceEqual[byte]($expected,$actual)) { throw "裁剪 PNG 在第 $row 行像素不一致" }
    }
    [pscustomobject]@{SourceSize=('{0}x{1}' -f $sourceBitmap.Width,$sourceBitmap.Height);RegionSize=('{0}x{1}' -f $width,$height);Origin=('{0},{1}' -f $left,$top);ComparedPixels=$width*$height;AllBgraPixelsMatch=$true}
} finally {
    if ($null -ne $sourceData) { $sourceBitmap.UnlockBits($sourceData) }
    if ($null -ne $regionData) { $regionBitmap.UnlockBits($regionData) }
    $sourceBitmap.Dispose()
    if ($null -ne $regionBitmap) { $regionBitmap.Dispose() }
}
