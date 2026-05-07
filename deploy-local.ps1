<#
.SYNOPSIS
  Ignite Pay — Windows Local Testing Deployment Script

.DESCRIPTION
  Build, start, stop, and manage all Ignite Pay backend services locally
  on Windows without Docker. Designed for development and local testing.

.EXAMPLE
  .\deploy-local.ps1 build       # Compile all Rust services (debug mode)
  .\deploy-local.ps1 start       # Start all services in background
  .\deploy-local.ps1 stop        # Stop all running services
  .\deploy-local.ps1 restart     # Restart all services
  .\deploy-local.ps1 status      # Show process status for all services
  .\deploy-local.ps1 logs router-user   # Tail log file for a service
  .\deploy-local.ps1 health      # Health-check all services
  .\deploy-local.ps1 clean       # Remove generated configs and data

.NOTES
  Requirements:
    - Rust toolchain (cargo)
    - PostgreSQL (for hub-registry service, or skip it)
    - PowerShell 5.1+
#>

param(
    [Parameter(Position=0)]
    [ValidateSet("build","start","stop","restart","status","logs","health","clean","help")]
    [string]$Command = "help",

    [Parameter(Position=1)]
    [string]$ServiceName = ""
)

$ErrorActionPreference = "Stop"

# --------------- Resolve Paths ---------------
$Script:SCRIPT_DIR = Split-Path -Parent $MyInvocation.MyCommand.Path
$ConfFile = Join-Path $Script:SCRIPT_DIR "deploy-local.conf.ps1"

if (-not (Test-Path $ConfFile)) {
    Write-Host "ERROR: $ConfFile not found. Copy deploy-local.conf.ps1 and edit it." -ForegroundColor Red
    exit 1
}

# Dot-source config
. $ConfFile

# Auto-detect project root
if (-not $PROJECT_ROOT) {
    $PROJECT_ROOT = $Script:SCRIPT_DIR
}

$BinDir = Join-Path $PROJECT_ROOT "target\debug"
$ConfDir = Join-Path $PROJECT_ROOT "local-config"
$DataDir = if ([System.IO.Path]::IsPathRooted($DATA_DIR)) { $DATA_DIR } else { Join-Path $PROJECT_ROOT $DATA_DIR }
$LogDir = Join-Path $PROJECT_ROOT "local-logs"
$PidDir = Join-Path $PROJECT_ROOT "local-pids"

# --------------- Service Definitions ---------------
# Format: @{ Name = "..."; Binary = "..."; Config = "..."; Port = N; Skip = $false }
$Script:SERVICES = @(
    @{ Name = "router-user";     Binary = "didcomm-router.exe";        Config = "router-user.toml";     Port = $ROUTER_USER_PORT }
    @{ Name = "router-merchant"; Binary = "didcomm-router.exe";        Config = "router-merchant.toml"; Port = $ROUTER_MERCHANT_PORT }
    @{ Name = "did-registry";    Binary = "did-registry.exe";          Config = "did-registry.toml";    Port = $DID_REGISTRY_PORT }
    @{ Name = "channel-user";    Binary = "channel-user.exe";          Config = "channel-user.toml";    Port = $CHANNEL_USER_PORT }
    @{ Name = "channel-provider";Binary = "channel-provider.exe";      Config = "channel-provider.toml";Port = $CHANNEL_PROVIDER_PORT }
    @{ Name = "channel-hub";     Binary = "channel-hub.exe";           Config = "channel-hub.toml";     Port = $CHANNEL_HUB_PORT }
    @{ Name = "hub-registry";    Binary = "ignite-pay-hub-registry.exe";Config = "hub-registry.toml";   Port = $HUB_REGISTRY_PORT }
)

# --------------- Helper Functions ---------------

function Ensure-Dir {
    param([string[]]$Paths)
    foreach ($p in $Paths) {
        if (-not (Test-Path $p)) { New-Item -ItemType Directory -Path $p -Force | Out-Null }
    }
}

function Get-PidFilePath {
    param([string]$Name)
    return Join-Path $PidDir "$Name.pid"
}

function Get-LogFilePath {
    param([string]$Name)
    return Join-Path $LogDir "$Name.log"
}

function Is-Running {
    param([string]$Name)
    $pidFile = Get-PidFilePath $Name
    if (-not (Test-Path $pidFile)) { return $false }
    $pid = Get-Content $pidFile -ErrorAction SilentlyContinue
    if (-not $pid) { return $false }
    try {
        $proc = Get-Process -Id $pid -ErrorAction SilentlyContinue
        if ($proc -and -not $proc.HasExited) { return $true }
    } catch { }
    return $false
}

function Stop-One {
    param([string]$Name)
    $pidFile = Get-PidFilePath $Name
    if (-not (Test-Path $pidFile)) { return }
    $pid = Get-Content $pidFile -ErrorAction SilentlyContinue
    if ($pid) {
        try {
            $proc = Get-Process -Id $pid -ErrorAction SilentlyContinue
            if ($proc -and -not $proc.HasExited) {
                Write-Host "  Stopping $Name (PID $pid)..." -ForegroundColor Yellow
                Stop-Process -Id $pid -Force -ErrorAction SilentlyContinue
                # Wait up to 5 seconds for process to exit
                $proc.WaitForExit(5000) | Out-Null
            }
        } catch { }
    }
    Remove-Item $pidFile -Force -ErrorAction SilentlyContinue
}

# --------------- Config Generation ---------------

function Generate-Configs {
    Write-Host "  Generating config files in $ConfDir\ ..."

    # Router - user
    @"
[server]
host = "0.0.0.0"
port = $ROUTER_USER_PORT

[router]
max_queued_messages = 1000
max_message_age_seconds = 86400

[storage]
path = "$($DataDir -replace '\\','/')\router-user"
"@ | Set-Content (Join-Path $ConfDir "router-user.toml") -Encoding UTF8

    # Router - merchant
    @"
[server]
host = "0.0.0.0"
port = $ROUTER_MERCHANT_PORT

[router]
max_queued_messages = 1000
max_message_age_seconds = 86400

[storage]
path = "$($DataDir -replace '\\','/')\router-merchant"
"@ | Set-Content (Join-Path $ConfDir "router-merchant.toml") -Encoding UTF8

    # DID Registry
    @"
[server]
host = "0.0.0.0"
port = $DID_REGISTRY_PORT

[solana]
rpc_url = "$SOLANA_RPC_URL"
did_program_id = "$DID_PROGRAM_ID"
payer_keypair_path = "$($DID_REGISTRY_PAYER_KEYPAIR -replace '\\','/')"

[auth]
jwt_secret = "$DID_REGISTRY_JWT_SECRET"
platform_public_key = "$PLATFORM_PUBLIC_KEY"
platform_signing_key_path = "$($PLATFORM_SIGNING_KEY_PATH -replace '\\','/')"

[fees]
register_fee_lamports = 5000
update_vc_fee_lamports = 2000
rotate_key_fee_lamports = 2000
"@ | Set-Content (Join-Path $ConfDir "did-registry.toml") -Encoding UTF8

    # Channel User
    @"
[server]
host = "0.0.0.0"
port = $CHANNEL_USER_PORT

[solana]
rpc_url = "$SOLANA_RPC_URL"
channel_program_id = "$CHANNEL_PROGRAM_ID"
keypair_path = "$($KEY_USER -replace '\\','/')"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "$($DataDir -replace '\\','/')\channel-user"

[compliance]
spending_threshold = 1000000000
per_channel_limit = 100000000
window_slots = 100000
travel_rule_threshold = 500000000
"@ | Set-Content (Join-Path $ConfDir "channel-user.toml") -Encoding UTF8

    # Channel Provider
    @"
[server]
host = "0.0.0.0"
port = $CHANNEL_PROVIDER_PORT

[solana]
rpc_url = "$SOLANA_RPC_URL"
channel_program_id = "$CHANNEL_PROGRAM_ID"
keypair_path = "$($KEY_PROVIDER -replace '\\','/')"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "$($DataDir -replace '\\','/')\channel-provider"
"@ | Set-Content (Join-Path $ConfDir "channel-provider.toml") -Encoding UTF8

    # Channel Hub
    @"
[server]
host = "0.0.0.0"
port = $CHANNEL_HUB_PORT

[solana]
rpc_url = "$SOLANA_RPC_URL"
channel_program_id = "$CHANNEL_PROGRAM_ID"
keypair_path = "$($KEY_HUB -replace '\\','/')"

[channel]
default_tree_depth = 4
default_challenge_duration = 5000
default_min_challenge_delay = 1000
default_settle_window = 10000
auto_close_offset = 500000
db_path = "$($DataDir -replace '\\','/')\channel-hub"

[compliance]
spending_threshold = 1000000000
per_channel_limit = 100000000
window_slots = 100000
travel_rule_threshold = 500000000
"@ | Set-Content (Join-Path $ConfDir "channel-hub.toml") -Encoding UTF8

    # Hub Registry
    @"
[server]
host = "0.0.0.0"
port = $HUB_REGISTRY_PORT

[database]
url = "$HUB_REGISTRY_DB_URL"
"@ | Set-Content (Join-Path $ConfDir "hub-registry.toml") -Encoding UTF8
}

# --------------- Commands ---------------

function Cmd-Build {
    Write-Host "===> Building all Rust services (debug mode)..." -ForegroundColor Cyan
    Write-Host ""

    Push-Location $PROJECT_ROOT
    try {
        $builds = @(
            @{ Name = "didcomm-router"; Dir = "didcomm-router" }
            @{ Name = "did-registry";   Dir = "did-registry" }
            @{ Name = "channel-service (user, provider, hub)"; Dir = "ignite-pay-channel-service" }
            @{ Name = "hub-registry";   Dir = "ignite-pay-hub-registry" }
        )

        $total = $builds.Count
        $i = 0
        foreach ($b in $builds) {
            $i++
            Write-Host "  [$i/$total] $($b.Name)" -ForegroundColor White
            $crateDir = Join-Path $PROJECT_ROOT $b.Dir
            & $CARGO_BIN build --manifest-path (Join-Path $crateDir "Cargo.toml") 2>&1 |
                ForEach-Object { Write-Host "    $_" }
            if ($LASTEXITCODE -ne 0) {
                Write-Host "  FAILED: $($b.Name)" -ForegroundColor Red
                Pop-Location
                exit 1
            }
        }

        Write-Host ""
        Write-Host "===> Build complete. Binaries in $BinDir\" -ForegroundColor Green
    } finally {
        Pop-Location
    }
}

function Cmd-Start {
    Ensure-Dir $ConfDir, $DataDir, $LogDir, $PidDir
    Ensure-Dir (Join-Path $DataDir "router-user"), (Join-Path $DataDir "router-merchant"), (Join-Path $DataDir "channel-user"), (Join-Path $DataDir "channel-provider"), (Join-Path $DataDir "channel-hub")

    # Always regenerate configs so port changes take effect
    Generate-Configs

    Write-Host "===> Starting Ignite Pay services..." -ForegroundColor Cyan
    Write-Host ""

    # Check for already-running services
    foreach ($svc in $SERVICES) {
        if (Is-Running $svc.Name) {
            Write-Host "  $($svc.Name) already running (skipping)" -ForegroundColor DarkGray
            continue
        }

        $binary = Join-Path $BinDir $svc.Binary
        if (-not (Test-Path $binary)) {
            Write-Host "  $($svc.Name) — binary not found: $binary" -ForegroundColor Red
            Write-Host "    Run '.\deploy-local.ps1 build' first." -ForegroundColor DarkGray
            continue
        }

        $config = Join-Path $ConfDir $svc.Config
        $logFile = Get-LogFilePath $svc.Name
        $pidFile = Get-PidFilePath $svc.Name

        # Start process in background, redirect output to log file
        $env:RUST_LOG = $RUST_LOG
        $proc = Start-Process -FilePath $binary -ArgumentList $config `
            -WindowStyle Hidden -PassThru `
            -RedirectStandardOutput (Join-Path $LogDir "$($svc.Name)-stdout.log") `
            -RedirectStandardError (Join-Path $LogDir "$($svc.Name)-stderr.log")

        # Give it a moment to see if it crashes immediately
        Start-Sleep -Milliseconds 500

        if ($proc.HasExited) {
            Write-Host "  $($svc.Name) — FAILED to start (exit code $($proc.ExitCode))" -ForegroundColor Red
            Write-Host "    See log: $logFile" -ForegroundColor DarkGray
            continue
        }

        $proc.Id | Set-Content $pidFile
        Write-Host "  $($svc.Name) started (PID $($proc.Id)) on port $($svc.Port)" -ForegroundColor Green
    }

    Write-Host ""
    Write-Host "===> All services launched. Use '.\deploy-local.ps1 status' to check." -ForegroundColor Cyan
}

function Cmd-Stop {
    Write-Host "===> Stopping all services..." -ForegroundColor Cyan
    foreach ($svc in $SERVICES) {
        if (Is-Running $svc.Name) {
            Stop-One $svc.Name
            Write-Host "  $($svc.Name) stopped" -ForegroundColor Green
        } else {
            Write-Host "  $($svc.Name) — not running" -ForegroundColor DarkGray
        }
    }
    Write-Host ""
    Write-Host "===> All services stopped." -ForegroundColor Cyan
}

function Cmd-Restart {
    Cmd-Stop
    Write-Host ""
    Cmd-Start
}

function Cmd-Status {
    Write-Host "===> Service Status" -ForegroundColor Cyan
    Write-Host ""
    $header = "{0,-20} {1,-8} {2,-8} {3,-10}" -f "SERVICE","PID","PORT","STATUS"
    Write-Host $header
    Write-Host ("-" * 50)

    foreach ($svc in $SERVICES) {
        $running = Is-Running $svc.Name
        if ($running) {
            $pid = Get-Content (Get-PidFilePath $svc.Name)
            $status = "RUNNING"
            $color = "Green"
        } else {
            $pid = "-"
            $status = "STOPPED"
            $color = "Red"
        }
        $line = "{0,-20} {1,-8} {2,-8} {3,-10}" -f $svc.Name, $pid, $svc.Port, $status
        Write-Host $line -ForegroundColor $color
    }
    Write-Host ""
}

function Cmd-Logs {
    param([string]$Name)

    if (-not $Name) {
        Write-Host "Usage: .\deploy-local.ps1 logs <service-name>" -ForegroundColor Yellow
        Write-Host ""
        Write-Host "Available services:"
        foreach ($svc in $SERVICES) {
            Write-Host "  $($svc.Name)" -ForegroundColor White
        }
        return
    }

    # Find matching service
    $found = $false
    foreach ($svc in $SERVICES) {
        if ($svc.Name -eq $Name) { $found = $true; break }
    }

    if (-not $found) {
        Write-Host "Unknown service: $Name" -ForegroundColor Red
        return
    }

    $stderrLog = Join-Path $LogDir "$Name-stderr.log"
    $stdoutLog = Join-Path $LogDir "$Name-stdout.log"

    if (Test-Path $stderrLog) {
        Write-Host "===> Logs for $Name (stderr):" -ForegroundColor Cyan
        Write-Host ""
        Get-Content $stderrLog -Tail 100
    }
    if (Test-Path $stdoutLog) {
        Write-Host ""
        Write-Host "===> Logs for $Name (stdout):" -ForegroundColor Cyan
        Write-Host ""
        Get-Content $stdoutLog -Tail 50
    }
    if ((-not (Test-Path $stderrLog)) -and (-not (Test-Path $stdoutLog))) {
        Write-Host "No log files found for $Name" -ForegroundColor Yellow
    }
}

function Cmd-Health {
    Write-Host "===> Health Check..." -ForegroundColor Cyan
    Write-Host ""

    foreach ($svc in $SERVICES) {
        $url = "http://127.0.0.1:$($svc.Port)/health"
        try {
            $response = Invoke-WebRequest -Uri $url -TimeoutSec 3 -UseBasicParsing -ErrorAction Stop
            Write-Host "  $($svc.Name) (:$($svc.Port)) — OK ($($response.StatusCode))" -ForegroundColor Green
        } catch {
            if (Is-Running $svc.Name) {
                Write-Host "  $($svc.Name) (:$($svc.Port)) — RUNNING but health check failed" -ForegroundColor Yellow
            } else {
                Write-Host "  $($svc.Name) (:$($svc.Port)) — NOT RUNNING" -ForegroundColor Red
            }
        }
    }
}

function Cmd-Clean {
    Write-Host "===> Cleaning local data, configs, logs, and PIDs..." -ForegroundColor Yellow
    Cmd-Stop
    Remove-Item $ConfDir -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $PidDir -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $LogDir -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $DataDir -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "  Cleaned: $ConfDir, $PidDir, $LogDir, $DataDir" -ForegroundColor Green
    Write-Host ""
    Write-Host "===> Done." -ForegroundColor Cyan
}

function Show-Help {
    Write-Host ""
    Write-Host "Ignite Pay — Windows Local Testing Deployment" -ForegroundColor Cyan
    Write-Host ""
    Write-Host "Usage: .\deploy-local.ps1 <command> [service-name]" -ForegroundColor White
    Write-Host ""
    Write-Host "Commands:"
    Write-Host "  build              Compile all Rust services (debug mode)"
    Write-Host "  start              Start all services in background"
    Write-Host "  stop               Stop all running services"
    Write-Host "  restart            Stop and restart all services"
    Write-Host "  status             Show process status for all services"
    Write-Host "  logs <service>     Show recent log output for a service"
    Write-Host "  health             Health-check all services via /health"
    Write-Host "  clean              Stop services and remove local data/configs/logs"
    Write-Host ""
    Write-Host "Services:"
    foreach ($svc in $SERVICES) {
        Write-Host "  $($svc.Name.PadRight(20)) port $($svc.Port)"
    }
    Write-Host ""
    Write-Host "Edit deploy-local.conf.ps1 before running 'start'." -ForegroundColor DarkGray
    Write-Host ""
}

# --------------- Main ---------------

switch ($Command) {
    "build"   { Cmd-Build }
    "start"   { Cmd-Start }
    "stop"    { Cmd-Stop }
    "restart" { Cmd-Restart }
    "status"  { Cmd-Status }
    "logs"    { Cmd-Logs $ServiceName }
    "health"  { Cmd-Health }
    "clean"   { Cmd-Clean }
    "help"    { Show-Help }
}
