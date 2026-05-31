# Fiuto

> *fiuto* in Italian means "sense of smell" or, figuratively, "intuition": the instinct for detecting something hidden or sizing up a situation quickly.

Fiuto is a CLI utility that drills an OpenAPI specification, firing every input combination it can build at your running backend so you can spot inconsistencies between the spec and the real service.

It was born from working on several services that ship an SDK and an OpenAPI spec: if you keep the spec up to date, a tool can read it and catch untested situations, broken input validation, or unwanted changes in responses before they reach users.

It is in an early stage.

## Installation

Build the binary with Nix:
```zsh
nix build
```
The binary lands at `./result/bin/fiuto`.

Or run it directly without installing:
```zsh
nix run . -- ./openapi.yml
```

## Usage

Point fiuto at a spec:
```zsh
fiuto ./openapi.yml
```
It drills every endpoint and prints a summary of the responses. With `--json` it also dumps the raw per-request results, which are easy to pipe into other tools.

Override or set the server base URL (useful when the spec points elsewhere):
```zsh
fiuto --base-url 'http://127.0.0.1:8001' ./openapi.yml
```
If a request cannot reach the URL, the tool stops at the first failure.

### Options

| Flag | Description |
| --- | --- |
| `--base-url <URL>` | Override the server base URL from the spec. |
| `--jwt <TOKEN>` | Send a `Bearer` token so endpoints behind auth can be tested. |
| `--skip-deprecated` | Skip endpoints marked deprecated in the spec. |
| `--json` | Print the raw per-request results as JSON before the summary. |
| `--delay <MILLIS>` | Wait this many milliseconds between requests. Default `0` (no wait). |
| `--delay-every <N>` | Apply `--delay` only once per `N` requests instead of after each one. Default `1`. |

### Throttling requests

Drilling fires requests back to back, so a rate-limited API answers with a flood of `429`s that bury the responses you care about. Slow the run down to stay under the limit:

```zsh
# wait 200ms between every request
fiuto --base-url 'http://127.0.0.1:8001' --delay 200 ./openapi.yml

# send 10 requests, then pause 1s, and repeat
fiuto --base-url 'http://127.0.0.1:8001' --delay 1000 --delay-every 10 ./openapi.yml
```

The request count is global across all endpoints. The pause is skipped before the first request and never trails the last one.

## Features

- [x] drill GET, POST and PUT endpoints
- [x] test every combination of input request
- [x] uses examples provided in the spec
- [x] json result easy to parse
- [x] support for full object example
- [x] support example for every property
- [x] skip deprecated endpoints with `--skip-deprecated`
- [x] send a request with a token using `--jwt <string>` (test endpoints behind auth)
- [x] throttle requests with `--delay` and `--delay-every` to avoid hitting rate limits

## Limitations

- only drills endpoints with `content: application/json`
- a POST or PUT request must have a `requestBody` with `$ref`
- the spec must contain a `components` section with the struct referenced above
- every `property` of the component schema needs an `example` (or a full example for the whole object); fiuto builds payloads from those `example` fields

## Development

Enter a development shell with all dependencies:
```zsh
nix develop
# then use standard Cargo commands:
cargo build
cargo test
cargo clippy
```

## Roadmap

- check responses
- support for headers to inject
- test combinations of headers
- test inputs other than the examples provided
- allow selecting a server from the spec `servers` list as base URL
- test `nullable` fields
- support GET with payload
- support POST with form bodies, for example:
```yaml
requestBody:
  required: true
  description: Form containing OPML file
  content:
    multipart/form-data:
      schema:
        type: object
        properties:
          file:
            type: string
            format: binary
```
