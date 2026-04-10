param(
  [string]$RepoName = "recall",
  [string]$Branch = "main",
  [ValidateSet("public", "private")]
  [string]$Visibility = "public",
  [string]$ReleaseNotes,
  [string]$SigningPrivateKeyPath
)

$ErrorActionPreference = "Stop"

function Write-Step {
  param([string]$Message)
  Write-Host ""
  Write-Host "==> $Message" -ForegroundColor Cyan
}

function Fail {
  param([string]$Message)
  throw "[release] $Message"
}

function Run {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Command,
    [string[]]$Arguments = @()
  )

  & $Command @Arguments
  if ($LASTEXITCODE -ne 0) {
    Fail "Command failed: $Command $($Arguments -join ' ')"
  }
}

function Test-CommandSuccess {
  param(
    [Parameter(Mandatory = $true)]
    [string]$Command,
    [string[]]$Arguments = @()
  )

  $previousErrorActionPreference = $ErrorActionPreference
  $ErrorActionPreference = "Continue"
  & $Command @Arguments *> $null
  $exitCode = $LASTEXITCODE
  $ErrorActionPreference = $previousErrorActionPreference
  return $exitCode -eq 0
}

function Has-Git-Changes {
  $status = git status --porcelain
  return -not [string]::IsNullOrWhiteSpace(($status -join ""))
}

function Has-Git-Commits {
  return Test-CommandSuccess "git" @("rev-parse", "--verify", "HEAD")
}

function Commit-If-Needed {
  param([string]$Message)

  if (Has-Git-Changes) {
    Run "git" @("add", ".")
    Run "git" @("commit", "-m", $Message)
  } else {
    Write-Host "No Git changes to commit for: $Message"
  }
}

function Ensure-Updater-Signing-Key {
  if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY) -and -not [string]::IsNullOrWhiteSpace($SigningPrivateKeyPath)) {
    if (-not (Test-Path -LiteralPath $SigningPrivateKeyPath)) {
      Fail "Signing private key path does not exist: $SigningPrivateKeyPath"
    }
    $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw -LiteralPath $SigningPrivateKeyPath
  }

  if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) {
    Fail "TAURI_SIGNING_PRIVATE_KEY is required because updater artifacts are enabled."
  }

  if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD)) {
    Fail "TAURI_SIGNING_PRIVATE_KEY_PASSWORD is required for the updater signing key."
  }
}

function Get-Gh-Login {
  $json = gh api user
  if ($LASTEXITCODE -ne 0) {
    Fail "gh is not authenticated. Run gh auth login and try again."
  }
  return ($json | ConvertFrom-Json).login
}

function Ensure-GitHub-Repo {
  param(
    [string]$Owner,
    [string]$Name
  )

  $repoFullName = "$Owner/$Name"
  $repoExists = Test-CommandSuccess "gh" @("repo", "view", $repoFullName)

  if (-not $repoExists) {
    Write-Step "Creating GitHub repo $repoFullName"
    $visibilityFlag = if ($Visibility -eq "public") { "--public" } else { "--private" }
    & gh repo create $Name $visibilityFlag --source=. --remote=origin *> $null
    if ($LASTEXITCODE -ne 0) {
      Fail "Command failed: gh repo create $Name $visibilityFlag --source=. --remote=origin"
    }
  } else {
    Write-Host "GitHub repo exists: $repoFullName"
    if (-not (Test-CommandSuccess "git" @("remote", "get-url", "origin"))) {
      Run "git" @("remote", "add", "origin", "https://github.com/$repoFullName.git")
    }
  }

  return "https://github.com/$repoFullName"
}

function Get-Project-Version {
  $tauriConfigPath = Join-Path $PWD "src-tauri\tauri.conf.json"
  if (Test-Path -LiteralPath $tauriConfigPath) {
    $tauriConfig = Get-Content -Raw -LiteralPath $tauriConfigPath | ConvertFrom-Json
    if (-not [string]::IsNullOrWhiteSpace($tauriConfig.version)) {
      return $tauriConfig.version
    }
  }

  $packageJson = Get-Content -Raw -LiteralPath "package.json" | ConvertFrom-Json
  return $packageJson.version
}

function Find-Latest-File {
  param(
    [string]$Path,
    [string]$Filter
  )

  $file = Get-ChildItem -Path $Path -Filter $Filter -File -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1

  if (-not $file) {
    Fail "Could not find $Filter in $Path"
  }

  return $file
}

Write-Step "Checking prerequisites"
Run "gh" @("--version")
Run "gh" @("auth", "status")
Run "git" @("--version")

$owner = Get-Gh-Login
$repoFullName = "$owner/$RepoName"

Write-Step "Preparing Git repository"
if (-not (Test-Path -LiteralPath ".git")) {
  Run "git" @("init")
}

$safeDirectory = (Resolve-Path -LiteralPath $PWD).Path.Replace("\", "/")
Run "git" @("config", "--global", "--add", "safe.directory", $safeDirectory)
Run "git" @("branch", "-M", $Branch)
if (Has-Git-Commits) {
  Commit-If-Needed "release setup"
} else {
  Commit-If-Needed "initial commit"
}

$repoUrl = Ensure-GitHub-Repo -Owner $owner -Name $RepoName
Run "git" @("push", "-u", "origin", $Branch)

Write-Step "Reading version"
$version = Get-Project-Version
$tag = "v$version"
if ([string]::IsNullOrWhiteSpace($ReleaseNotes)) {
  $ReleaseNotes = "Release $tag"
}
Write-Host "Version: $version"

Write-Step "Building signed Tauri app"
Ensure-Updater-Signing-Key
Run "npm.cmd" @("run", "tauri:build")

Write-Step "Locating Windows artifacts"
$msiDir = Join-Path $PWD "src-tauri\target\release\bundle\msi"
$msi = Find-Latest-File -Path $msiDir -Filter "*.msi"
$sig = Find-Latest-File -Path $msiDir -Filter "*.sig"
Write-Host "MSI: $($msi.FullName)"
Write-Host "Signature: $($sig.FullName)"

Write-Step "Creating or updating GitHub release"
$releaseExists = Test-CommandSuccess "gh" @("release", "view", $tag)
if ($releaseExists) {
  Run "gh" @("release", "upload", $tag, $msi.FullName, $sig.FullName, "--clobber")
  Run "gh" @("release", "edit", $tag, "--title", "Recall $tag", "--notes", $ReleaseNotes)
} else {
  Run "gh" @("release", "create", $tag, $msi.FullName, $sig.FullName, "--title", "Recall $tag", "--notes", $ReleaseNotes, "--target", $Branch)
}

$releaseJson = gh release view $tag --json url
if ($LASTEXITCODE -ne 0) {
  Fail "Unable to read GitHub release URL."
}
$releaseUrl = ($releaseJson | ConvertFrom-Json).url
$msiUrl = "https://github.com/$repoFullName/releases/download/$tag/$($msi.Name)"

Write-Step "Writing Tauri updater manifest"
$signature = (Get-Content -Raw -LiteralPath $sig.FullName).Trim()
$updatesDir = Join-Path $PWD "updates"
New-Item -ItemType Directory -Force -Path $updatesDir | Out-Null

$manifest = [ordered]@{
  version = $version
  notes = $ReleaseNotes
  pub_date = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
  platforms = [ordered]@{
    "windows-x86_64" = [ordered]@{
      signature = $signature
      url = $msiUrl
    }
  }
}

$manifestPath = Join-Path $updatesDir "latest.json"
$manifest | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $manifestPath -Encoding UTF8

Commit-If-Needed "update manifest for $tag"
Run "git" @("push", "origin", $Branch)

$manifestUrl = "https://raw.githubusercontent.com/$repoFullName/$Branch/updates/latest.json"

Write-Host ""
Write-Host "Release pipeline complete." -ForegroundColor Green
Write-Host "Repo URL:      $repoUrl"
Write-Host "Release URL:   $releaseUrl"
Write-Host "Manifest URL:  $manifestUrl"
Write-Host "Version:       $version"
