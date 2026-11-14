# ReleaseHub local workboard client. ASCII-only for Windows PowerShell 5.1.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('health', 'projects', 'tasks', 'check', 'claim', 'progress', 'result')]
    [string]$Action,
    [string]$BaseUrl = 'http://127.0.0.1:47831',
    [string]$TaskId,
    [string]$AgentName = 'local-agent',
    [string]$Message,
    [switch]$Failed,
    [string]$ArtifactUrl,
    [string]$SourceCommit,
    [string]$Sha256
)
$ErrorActionPreference = 'Stop'
$uri = [uri]$BaseUrl
if ($uri.Scheme -ne 'http' -or $uri.Host -notin @('127.0.0.1', 'localhost') -or $uri.Port -lt 1024 -or $uri.UserInfo -or $uri.Query -or $uri.Fragment -or $uri.AbsolutePath -ne '/') {
    throw 'BaseUrl must be a direct loopback HTTP origin with a non-privileged port.'
}
$token = $env:RELEASEHUB_AGENT_TOKEN
if ([string]::IsNullOrWhiteSpace($token)) {
    throw 'Set RELEASEHUB_AGENT_TOKEN in this process first. Never save it in source files or public logs.'
}
$headers = @{ Authorization = "Bearer $token" }
$method = 'GET'
$body = $null
$route = '/v1/health'
switch ($Action) {
    'projects' { $route = '/v1/projects' }
    'tasks' { $route = '/v1/build-tasks' }
    'check' { $route = '/v1/check-updates'; $method = 'POST'; $body = @{} }
    { $_ -in @('claim', 'progress', 'result') } {
        if ($TaskId -notmatch '^[A-Za-z0-9-]{1,100}$') { throw 'A valid TaskId is required.' }
        $route = "/v1/build-tasks/$TaskId/$Action"
        $method = 'POST'
        if ($Action -eq 'claim') {
            $body = @{ agentName = $AgentName }
        } else {
            if ([string]::IsNullOrWhiteSpace($env:RELEASEHUB_CLAIM_TOKEN)) { throw 'Set RELEASEHUB_CLAIM_TOKEN from the claim response for this task.' }
            $headers['X-ReleaseHub-Claim'] = $env:RELEASEHUB_CLAIM_TOKEN
            if ($Action -eq 'progress') {
                $status = 'running'
                if ($Failed) { $status = 'failed' }
                $body = @{ status = $status; progress = $Message }
                if ($Failed) { $body.error = $Message }
            } else {
                if (-not $ArtifactUrl -or $SourceCommit -notmatch '^([a-fA-F0-9]{40}|[a-fA-F0-9]{64})$') { throw 'A GitHub artifact URL and full source commit are required.' }
                $body = @{ artifactUrl = $ArtifactUrl; sourceCommit = $SourceCommit; progress = $Message }
                if ($Sha256) { $body.artifactSha256 = $Sha256 }
            }
        }
    }
}
$params = @{ Uri = "$($BaseUrl.TrimEnd('/'))$route"; Method = $method; Headers = $headers; TimeoutSec = 90 }
if ($null -ne $body) {
    $params.ContentType = 'application/json; charset=utf-8'
    $params.Body = [System.Text.Encoding]::UTF8.GetBytes(($body | ConvertTo-Json -Depth 8 -Compress))
}
# Return objects. Capture claim responses in a variable to avoid printing tickets.
Invoke-RestMethod @params
