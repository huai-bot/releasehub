# ReleaseHub local workboard client. ASCII-only for Windows PowerShell 5.1.
<#
.SYNOPSIS
Call the ReleaseHub local Agent API without additional dependencies.
.DESCRIPTION
Use help or schema without a running application. Use -Json for one JSON
result and a stable process exit code. Otherwise return PowerShell objects
for compatibility. Secrets come only from process environment variables.
.EXAMPLE
powershell.exe -NoProfile -File .\releasehub-agent.ps1 tasks -Status queued -Json
.EXAMPLE
$claim = & .\releasehub-agent.ps1 claim -TaskId $id -AgentName 'my-agent'
# Capture, do not log, $claim.claimToken before reporting progress or results.
#>
[CmdletBinding()]
param(
    [Parameter(Position = 0)][string]$Action = 'help',
    [string]$BaseUrl,
    [string]$TaskId,
    [string[]]$ProjectIds,
    [string]$Status,
    [string]$Query,
    [string]$AgentName = 'local-agent',
    [string]$Message,
    [switch]$Failed,
    [string]$ArtifactUrl,
    [string]$SourceCommit,
    [string]$Sha256,
    [int]$TimeoutSec = 90,
    [int]$WaitTimeoutSec = 300,
    [int]$PollIntervalSec = 3,
    [switch]$Json
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$client = $null
$handler = $null
$data = $null
$token = $env:RELEASEHUB_AGENT_TOKEN
$claimToken = $env:RELEASEHUB_CLAIM_TOKEN
$Action = $Action.ToLowerInvariant()
if ($Json) { [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false) }

function Stop-Client([string]$Code, [string]$Text, [int]$ExitCode = 2, [int]$HttpStatus = 0, [bool]$OutcomeUnknown = $false) {
    $exception = New-Object System.Exception($Text)
    $exception.Data['code'] = $Code
    $exception.Data['exitCode'] = $ExitCode
    $exception.Data['httpStatus'] = $HttpStatus
    $exception.Data['outcomeUnknown'] = $OutcomeUnknown
    throw $exception
}

function Get-SafeMessage([string]$Text) {
    foreach ($secret in @($token, $claimToken)) {
        if (-not [string]::IsNullOrWhiteSpace($secret)) { $Text = $Text.Replace($secret, '[redacted]') }
    }
    $Text = [regex]::Replace($Text, '[\x00-\x1f\x7f]', ' ')
    if ($Text.Length -gt 1000) { $Text = $Text.Substring(0, 1000) }
    return $Text
}

function Invoke-LocalApi([string]$Route, [string]$Method = 'GET', $Body = $null, [int]$BudgetMs = ($TimeoutSec * 1000), [switch]$List) {
    $request = New-Object System.Net.Http.HttpRequestMessage([System.Net.Http.HttpMethod]::new($Method), "$origin$Route")
    $response = $null
    $deadline = New-Object System.Threading.CancellationTokenSource
    $deadline.CancelAfter($BudgetMs)
    try {
        $request.Headers.Authorization = New-Object System.Net.Http.Headers.AuthenticationHeaderValue('Bearer', $token)
        if ($Action -in @('progress', 'result')) { $request.Headers.Add('X-ReleaseHub-Claim', $claimToken) }
        if ($null -ne $Body) {
            $request.Content = New-Object System.Net.Http.StringContent(($Body | ConvertTo-Json -Depth 8 -Compress), [Text.Encoding]::UTF8, 'application/json')
        }
        try {
            $response = $client.SendAsync($request, $deadline.Token).GetAwaiter().GetResult()
            $text = $response.Content.ReadAsStringAsync().GetAwaiter().GetResult()
        } catch {
            # Never echo exceptions containing headers, URLs or request bodies.
            if ($deadline.IsCancellationRequested) {
                Stop-Client 'request_timeout' 'The local API request timed out. Inspect current state before repeating a write.' 3 0 ($Method -ne 'GET')
            }
            Stop-Client 'connection_failed' 'Cannot reach the local API. Open ReleaseHub and enable Agent access; check its port.' 3 0 ($Method -ne 'GET')
        }
        $statusCode = [int]$response.StatusCode
        if ($statusCode -ge 300 -and $statusCode -lt 400) {
            Stop-Client 'redirect_refused' 'The local API returned a redirect. No redirect was followed.' 5 $statusCode ($Method -ne 'GET')
        }
        if (-not $response.IsSuccessStatusCode) {
            $messageText = 'The local API refused the request.'
            try {
                $errorBody = ConvertFrom-Json -InputObject $text
                if ($errorBody.error -is [string]) { $messageText = Get-SafeMessage $errorBody.error }
            } catch { }
            if ($statusCode -in @(401, 403)) { Stop-Client 'authentication_failed' $messageText 4 $statusCode }
            if ($statusCode -eq 409) { Stop-Client 'conflict' $messageText 5 $statusCode }
            if ($statusCode -eq 429) { Stop-Client 'rate_limited' $messageText 5 $statusCode }
            Stop-Client 'api_error' $messageText 5 $statusCode ($Method -ne 'GET' -and $statusCode -ge 500)
        }
        try {
            $trimmed = $text.Trim()
            if (($List -and -not $trimmed.StartsWith('[')) -or (-not $List -and -not $trimmed.StartsWith('{'))) { throw 'Unexpected JSON shape.' }
            $result = ConvertFrom-Json -InputObject $text
            if ($List) { if ($null -eq $result) { $result = @() } else { $result = @($result) } }
        } catch { Stop-Client 'invalid_response' 'The local API did not return the expected JSON response.' 5 $statusCode ($Method -ne 'GET') }
        return ,$result
    } finally {
        if ($null -ne $response) { $response.Dispose() }
        $request.Dispose()
        $deadline.Dispose()
    }
}

try {
    $commands = [ordered]@{
        help = @(); schema = @(); health = @();
        projects = @('Query'); tasks = @('Status', 'Query');
        task = @('TaskId'); check = @('ProjectIds');
        claim = @('TaskId', 'AgentName'); progress = @('TaskId', 'Message', 'Failed');
        result = @('TaskId', 'ArtifactUrl', 'SourceCommit', 'Sha256', 'Message');
        wait = @('TaskId', 'WaitTimeoutSec', 'PollIntervalSec')
    }
    if (-not $commands.Contains($Action)) { Stop-Client 'invalid_arguments' 'Unknown action. Run help or schema to list commands.' }
    $common = @('Action', 'BaseUrl', 'TimeoutSec', 'Json', 'Verbose', 'Debug', 'ErrorAction', 'WarningAction', 'InformationAction', 'ErrorVariable', 'WarningVariable', 'InformationVariable', 'OutVariable', 'OutBuffer', 'PipelineVariable')
    foreach ($key in $PSBoundParameters.Keys) {
        if ($key -notin $common -and $key -notin $commands[$Action]) { Stop-Client 'invalid_arguments' "Parameter -$key is not supported by this action." }
    }
    if ($TimeoutSec -lt 1 -or $TimeoutSec -gt 600) { Stop-Client 'invalid_arguments' 'TimeoutSec must be between 1 and 600.' }
    $description = [ordered]@{
        schemaVersion = 1
        name = 'releasehub-agent'
        commands = $commands
        requiredParameters = @{ task = @('TaskId'); claim = @('TaskId'); progress = @('TaskId', 'Message'); result = @('TaskId', 'ArtifactUrl', 'SourceCommit'); wait = @('TaskId') }
        taskStatuses = @('queued', 'claimed', 'running', 'succeeded', 'failed', 'cancelled')
        environment = @('RELEASEHUB_AGENT_URL (optional)', 'RELEASEHUB_AGENT_TOKEN', 'RELEASEHUB_CLAIM_TOKEN (progress/result only)')
        jsonEnvelope = '{schemaVersion, ok, action, data, error: null|{code,message,httpStatus,outcomeUnknown}}'
        exitCodes = @{ '0' = 'success'; '2' = 'arguments/configuration'; '3' = 'connection/request timeout'; '4' = 'authentication/claim ticket'; '5' = 'API/protocol/conflict'; '6' = 'wait: task failed/cancelled'; '7' = 'wait deadline'; '8' = 'task not found' }
        limits = @{ projectIds = 200; messageUtf8Bytes = 4000; failedMessageUtf8Bytes = 2000; sourceCommitHexDigits = 40; sha256HexDigits = 64; waitSeconds = '1..7200'; pollSeconds = '2..60'; requestTimeoutSeconds = '1..600' }
        safety = @('loopback only; no proxy or redirects', 'no automatic retries of write requests', 'check never downloads or installs', 'claim response contains a secret ticket: capture it, do not log it', 'create/cancel tasks and install through the desktop UI')
    }
    if ($Action -in @('help', 'schema')) {
        if ($Action -eq 'help' -and -not $Json) {
            @'
ReleaseHub Agent CLI (PowerShell 5.1 / 7; no extra dependencies)
Usage: releasehub-agent.ps1 [action] [parameters] [-Json]

help / schema                 Offline help / machine-readable command description
health                        Check local API availability and authentication
projects [-Query text]        List projects (name/repository match)
tasks [-Status queued]        List tasks, optionally filter by status and -Query
task -TaskId id               Read one task
check [-ProjectIds id1,id2]   Check selected projects, or all when omitted
claim -TaskId id              Claim a queued task; optional -AgentName
progress -TaskId id -Message text [-Failed]
                              Start/update progress, or report failure
result -TaskId id -ArtifactUrl url -SourceCommit sha [-Sha256 hash] [-Message text]
                              Report result AFTER progress has started the task
wait -TaskId id [-WaitTimeoutSec 300] [-PollIntervalSec 3]
                              Wait for success/failure/cancellation (read-only)

Common: -BaseUrl http://127.0.0.1:47831  -TimeoutSec 90  -Json
Environment: RELEASEHUB_AGENT_URL (optional), RELEASEHUB_AGENT_TOKEN,
             RELEASEHUB_CLAIM_TOKEN (progress/result only).
Default output is a PowerShell object. -Json emits one JSON envelope.
JSON errors include an error code and outcomeUnknown for uncertain writes.
Exit codes: 0 success; 2 usage/config; 3 connection; 4 auth; 5 API;
            6 task failed/cancelled; 7 wait deadline; 8 task not found.
Secrets are not command-line parameters. Capture claim output; do not log it.
No command executes source code or installs software. Never blindly retry writes.
'@ | Write-Output
            exit 0
        }
        $data = $description
    } else {
        if (-not $BaseUrl) { $BaseUrl = $env:RELEASEHUB_AGENT_URL }
        if (-not $BaseUrl) { $BaseUrl = 'http://127.0.0.1:47831' }
        $uri = $null
        if (-not [uri]::TryCreate($BaseUrl, [UriKind]::Absolute, [ref]$uri) -or $uri.Scheme -ne 'http' -or $uri.Host -notin @('127.0.0.1', 'localhost') -or $uri.Port -lt 1024 -or $uri.UserInfo -or $uri.Query -or $uri.Fragment -or $uri.AbsolutePath -ne '/') {
            Stop-Client 'invalid_configuration' 'BaseUrl must be a direct loopback HTTP origin with a non-privileged port.'
        }
        # Normalize localhost to IPv4 loopback: no DNS, proxy or alternate local address.
        $origin = "http://127.0.0.1:$($uri.Port)"
        if ($token -cnotmatch '^[a-fA-F0-9]{64}$') { Stop-Client 'invalid_configuration' 'Set a valid RELEASEHUB_AGENT_TOKEN in this process. Never save it in source or logs.' }
        if ($Action -in @('task', 'claim', 'progress', 'result', 'wait') -and $TaskId -cnotmatch '^[A-Za-z0-9-]{1,100}$') { Stop-Client 'invalid_arguments' 'A valid TaskId is required.' }
        if ($Action -eq 'tasks' -and $Status -and $Status -notin $description.taskStatuses) { Stop-Client 'invalid_arguments' 'Invalid task status. Run schema for allowed values.' }
        if ($Query.Length -gt 500) { Stop-Client 'invalid_arguments' 'Query must be at most 500 characters.' }
        if ($Action -eq 'check' -and $PSBoundParameters.ContainsKey('ProjectIds')) {
            # -File passes comma-separated IDs as one string; also accept native PS arrays.
            $ProjectIds = @($ProjectIds | ForEach-Object { $_.Split(',') } | ForEach-Object { $_.Trim() })
            if (-not $ProjectIds -or $ProjectIds.Count -gt 200) { Stop-Client 'invalid_arguments' 'Provide between 1 and 200 project IDs, or omit ProjectIds to check all.' }
            foreach ($id in $ProjectIds) { if ($id -cnotmatch '^[A-Za-z0-9-]{1,128}$') { Stop-Client 'invalid_arguments' 'ProjectIds must contain valid IDs from the projects command.' } }
        }
        if ($Action -eq 'claim' -and ([string]::IsNullOrWhiteSpace($AgentName) -or [Text.Encoding]::UTF8.GetByteCount($AgentName) -gt 100)) { Stop-Client 'invalid_arguments' 'AgentName must contain 1 to 100 UTF-8 bytes.' }
        if ($Action -in @('progress', 'result')) {
            if ($claimToken -cnotmatch '^[a-fA-F0-9]{64}$') { Stop-Client 'invalid_configuration' 'Set RELEASEHUB_CLAIM_TOKEN from this task claim response.' }
            $messageLimit = 4000
            if ($Failed) { $messageLimit = 2000 }
            if (($Action -eq 'progress' -and [string]::IsNullOrWhiteSpace($Message)) -or [Text.Encoding]::UTF8.GetByteCount($Message) -gt $messageLimit) { Stop-Client 'invalid_arguments' "Message must be a short summary (maximum $messageLimit UTF-8 bytes); progress requires a nonempty message." }
        }
        if ($Action -eq 'result') {
            # The core currently accepts GitHub SHA-1 commits, even though the HTTP parser also accepts 64 digits.
            if ($SourceCommit -cnotmatch '^[a-fA-F0-9]{40}$') { Stop-Client 'invalid_arguments' 'SourceCommit must be the full 40-digit commit pinned by this task.' }
            if ($Sha256 -and $Sha256 -cnotmatch '^[a-fA-F0-9]{64}$') { Stop-Client 'invalid_arguments' 'Sha256 must contain exactly 64 hexadecimal digits.' }
            $artifact = $null
            if (-not [uri]::TryCreate($ArtifactUrl, [UriKind]::Absolute, [ref]$artifact) -or $artifact.Scheme -ne 'https' -or $artifact.Host -ne 'github.com' -or -not $artifact.IsDefaultPort -or $artifact.UserInfo -or $artifact.Query -or $artifact.Fragment -or $artifact.AbsolutePath -notmatch '^/[^/]+/[^/]+/(releases/.+|actions/runs/[^/]+.*)$') {
                Stop-Client 'invalid_arguments' 'ArtifactUrl must be a credential-free HTTPS GitHub release or Actions run URL without query or fragment.'
            }
        }
        if ($Action -eq 'wait' -and ($WaitTimeoutSec -lt 1 -or $WaitTimeoutSec -gt 7200 -or $PollIntervalSec -lt 2 -or $PollIntervalSec -gt 60)) { Stop-Client 'invalid_arguments' 'WaitTimeoutSec must be 1..7200 and PollIntervalSec 2..60.' }
        Add-Type -AssemblyName System.Net.Http
        $handler = New-Object System.Net.Http.HttpClientHandler
        $handler.UseProxy = $false
        $handler.AllowAutoRedirect = $false
        $client = New-Object System.Net.Http.HttpClient($handler)
        $client.Timeout = [Threading.Timeout]::InfiniteTimeSpan
        $client.MaxResponseContentBufferSize = 8 * 1024 * 1024
        switch ($Action) {
            'health' { $data = Invoke-LocalApi '/v1/health' }
            'projects' { $data = Invoke-LocalApi '/v1/projects' -List; if ($Query) { $data = @($data | Where-Object { ("$($_.name) $($_.repository)").IndexOf($Query, [StringComparison]::OrdinalIgnoreCase) -ge 0 }) } }
            'tasks' { $data = Invoke-LocalApi '/v1/build-tasks' -List; if ($Status) { $data = @($data | Where-Object { $_.status -eq $Status }) }; if ($Query) { $data = @($data | Where-Object { ("$($_.projectName) $($_.repository) $($_.id)").IndexOf($Query, [StringComparison]::OrdinalIgnoreCase) -ge 0 }) } }
            'task' { $tasks = Invoke-LocalApi '/v1/build-tasks' -List; $data = $tasks | Where-Object { $_.id -eq $TaskId }; if ($null -eq $data) { Stop-Client 'task_not_found' 'Task not found. Refresh the task list.' 8 } }
            'check' { $body = @{}; if ($PSBoundParameters.ContainsKey('ProjectIds')) { $body.projectIds = @($ProjectIds) }; $data = Invoke-LocalApi '/v1/check-updates' 'POST' $body }
            'claim' { $data = Invoke-LocalApi "/v1/build-tasks/$TaskId/claim" 'POST' @{ agentName = $AgentName } }
            'progress' { $body = @{ status = 'running'; progress = $Message }; if ($Failed) { $body.status = 'failed'; $body.error = $Message }; $data = Invoke-LocalApi "/v1/build-tasks/$TaskId/progress" 'POST' $body }
            'result' { $body = @{ artifactUrl = $ArtifactUrl; sourceCommit = $SourceCommit; progress = $Message }; if ($Sha256) { $body.artifactSha256 = $Sha256 }; $data = Invoke-LocalApi "/v1/build-tasks/$TaskId/result" 'POST' $body }
            'wait' {
                $clock = [Diagnostics.Stopwatch]::StartNew()
                while ($true) {
                    $remaining = [int]($WaitTimeoutSec * 1000 - $clock.ElapsedMilliseconds)
                    if ($remaining -le 0) { Stop-Client 'wait_timeout' 'Wait deadline reached. The task was not cancelled; inspect it or wait again.' 7 }
                    $tasks = Invoke-LocalApi '/v1/build-tasks' -List -BudgetMs ([Math]::Min($remaining, $TimeoutSec * 1000))
                    $data = $tasks | Where-Object { $_.id -eq $TaskId }
                    if ($null -eq $data) { Stop-Client 'task_not_found' 'Task not found. Refresh the task list.' 8 }
                    if ($data.status -eq 'succeeded') { break }
                    if ($data.status -in @('failed', 'cancelled')) { Stop-Client 'task_unsuccessful' 'The task ended without success. See data for its final state.' 6 }
                    $remaining = [int]($WaitTimeoutSec * 1000 - $clock.ElapsedMilliseconds)
                    if ($remaining -gt 0) { Start-Sleep -Milliseconds ([Math]::Min($remaining, $PollIntervalSec * 1000)) }
                }
            }
        }
    }
    if ($Json) { [ordered]@{ schemaVersion = 1; ok = $true; action = $Action; data = $data; error = $null } | ConvertTo-Json -Depth 32 -Compress }
    else { Write-Output $data }
    exit 0
} catch {
    $exception = $_.Exception
    $code = 'client_error'; $exitCode = 5; $httpStatus = 0; $outcomeUnknown = $false
    $text = 'The client could not complete the request. Check parameters and local API availability.'
    if ($exception.Data.Contains('exitCode')) {
        $code = $exception.Data['code']; $exitCode = $exception.Data['exitCode']; $httpStatus = $exception.Data['httpStatus']; $outcomeUnknown = $exception.Data['outcomeUnknown']; $text = Get-SafeMessage $exception.Message
    }
    if ($Json) { [ordered]@{ schemaVersion = 1; ok = $false; action = $Action; data = $data; error = @{ code = $code; message = $text; httpStatus = $httpStatus; outcomeUnknown = $outcomeUnknown } } | ConvertTo-Json -Depth 32 -Compress }
    else { Write-Error -Message "$code`: $text" -ErrorAction Continue }
    exit $exitCode
} finally {
    if ($null -ne $client) { $client.Dispose() }
    if ($null -ne $handler) { $handler.Dispose() }
}
