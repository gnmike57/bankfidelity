Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$dir = 'C:\bankfidelity\bankfidelity\audit-evidence\xray-screenshots'
if (!(Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir }
$proc = Start-Process 'C:\bankfidelity\bankfidelity\target\debug\dual-core-pdf-pipeline.exe' -ArgumentList 'gui' -PassThru
Start-Sleep -Seconds 3
for ($i=1; $i -le 10; $i++) {
    $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
    $bmp = New-Object System.Drawing.Bitmap $bounds.width, $bounds.height
    $graphics = [System.Drawing.Graphics]::FromImage($bmp)
    $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.size)
    $bmp.Save("$dir\screenshot_$i.png", [System.Drawing.Imaging.ImageFormat]::Png)
    $graphics.Dispose()
    $bmp.Dispose()
    Start-Sleep -Seconds 1
}
Stop-Process -Id $proc.Id
