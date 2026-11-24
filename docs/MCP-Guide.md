# DTool 0.1 MCP 연결 가이드

## DTool에서 켜기

1. AI 클라이언트와 같은 Windows PC에서 DTool을 실행합니다.
2. **편집 → 설정 → 모델 컨텍스트 프로토콜**에서 **DTool MCP 켜기**를 확인합니다.
3. `%APPDATA%\DTool\mcp_endpoint.json`의 `addr`가 현재 주소, `token`이 인증 토큰입니다. DTool을 다시 실행하거나 MCP를 다시 켜면 두 값이 바뀔 수 있습니다.

## 로컬 HTTP 연결

클라이언트에서 URL을 `http://<addr>/`로 설정하고, JSON-RPC 요청마다 `Authorization: Bearer <token>` 헤더를 보냅니다. DTool 0.1은 로컬 HTTP의 JSON 요청·응답을 제공합니다. 클라이언트가 stdio만 지원하거나 별도의 스트리밍 세션을 요구한다면 클라이언트 쪽에 호환 브리지가 필요합니다.

DTool은 루프백 주소에 연결을 받습니다. 토큰을 채팅, GitHub 또는 공유 설정 파일에 넣지 마세요. 같은 PC에서 실제로 필요한 앱에만 제공하세요.

## PowerShell에서 연결 확인

DTool이 열린 상태에서 다음 코드를 실행합니다. 토큰을 터미널에 출력하지 않고 서버 정보와 도구 이름을 확인할 수 있습니다.

```powershell
$endpointPath = Join-Path $env:APPDATA 'DTool\mcp_endpoint.json'
$endpoint = Get-Content -LiteralPath $endpointPath -Raw | ConvertFrom-Json
if ($endpoint.stopped -or -not $endpoint.addr -or -not $endpoint.token) {
    throw 'DTool에서 MCP를 켜고 다시 시도하세요.'
}
$uri = "http://$($endpoint.addr)/"
$headers = @{ Authorization = "Bearer $($endpoint.token)" }
function Invoke-DToolMcp($id, $method, $params) {
    $body = @{
        jsonrpc = '2.0'
        id = $id
        method = $method
        params = $params
    } | ConvertTo-Json -Depth 8 -Compress
    $response = Invoke-RestMethod -Uri $uri -Method Post -Headers $headers `
        -ContentType 'application/json' -Body $body
    if ($response.error) { throw ($response.error | ConvertTo-Json -Compress) }
    $response.result
}
Invoke-DToolMcp 1 'initialize' @{
    protocolVersion = '2026-07-28'
    capabilities = @{}
    clientInfo = @{ name = 'DTool PowerShell Check'; version = '1' }
} | Select-Object protocolVersion, serverInfo
(Invoke-DToolMcp 2 'tools/list' @{}).tools | Select-Object name
```

## 연결이 끊겼을 때

- 파일이 없거나 연결이 거부되면 DTool이 실행 중인지, 설정에서 MCP를 켰는지 확인합니다.
- HTTP 401이 나오면 DTool을 다시 연 뒤 만들어진 주소와 토큰을 다시 읽습니다.
- 파일 열기나 저장 같은 요청이 거절되면 DTool의 MCP 권한과 앱에 표시되는 승인 요청을 확인합니다. 필요한 권한만 켭니다.
