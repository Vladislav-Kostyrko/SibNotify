param(
    [string]$Webhook = "",
    [int]$UserId = 4157,
    [string]$MmToken = "",
    [string]$Ics = "",
    [switch]$Uninstall
)

$ErrorActionPreference = "Stop"
$dir = Join-Path $env:LOCALAPPDATA "SibNotify"
$cfgDir = Join-Path $env:APPDATA "ru.sibtrans.notify"
$startup = [Environment]::GetFolderPath('Startup')
$lnkPath = Join-Path $startup "SibNotify.lnk"

if ($Uninstall) {
    Get-Process SibNotify -ErrorAction SilentlyContinue | Stop-Process -Force
    Remove-Item $lnkPath -Force -ErrorAction SilentlyContinue
    Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "SibNotify удалён (конфиг остался в $cfgDir)"
    exit 0
}

New-Item -ItemType Directory -Force -Path $dir | Out-Null
New-Item -ItemType Directory -Force -Path $cfgDir | Out-Null

$src = Join-Path $PSScriptRoot "SibNotify.exe"
if (-not (Test-Path $src)) { Write-Error "Рядом со скриптом нет SibNotify.exe"; exit 1 }
Copy-Item $src (Join-Path $dir "SibNotify.exe") -Force

if ($Webhook -ne "") {
    $cfg = [ordered]@{
        webhook          = $Webhook
        user_id          = $UserId
        refresh_secs     = 60
        panel_width      = 400
        strip_width      = 6
        monitor_index    = 0
        show_system      = $true
        show_tasks       = $true
        show_comments    = $true
        deadline_filter  = "all"
        tasks_limit      = 12
        comments_per_task = 3
        opacity          = 92
        mm_url           = "https://mm.sibtrans.ru"
        mm_token         = $MmToken
        ics_urls         = $Ics
        tg_token         = ""
        weather_city     = "Новосибирск"
        weather_lat      = 55.03
        weather_lon      = 82.92
        show_cal         = $true
        show_mm          = ($MmToken -ne "")
        show_chats       = $true
        show_weather     = $true
        sound            = $true
        panel_side       = "left"
        blocks           = @()
    }
    $cfg | ConvertTo-Json | Set-Content -Path (Join-Path $cfgDir "config.json") -Encoding UTF8
}

$ws = New-Object -ComObject WScript.Shell
$lnk = $ws.CreateShortcut($lnkPath)
$lnk.TargetPath = Join-Path $dir "SibNotify.exe"
$lnk.WorkingDirectory = $dir
$lnk.Description = "Sibtrans notification panel"
$lnk.Save()

Get-Process SibNotify -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Milliseconds 400
Start-Process (Join-Path $dir "SibNotify.exe")
Start-Sleep -Seconds 3
$p = Get-Process SibNotify -ErrorAction SilentlyContinue
if ($p) {
    Write-Host ("SibNotify запущен, PID {0}, память {1:N0} МБ" -f $p.Id, ($p.WorkingSet64 / 1MB))
} else {
    Write-Host "SibNotify НЕ запустился — смотри журнал в $cfgDir или запусти exe руками"
}
