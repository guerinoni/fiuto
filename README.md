# Fiuto

> fiuto in italian means "sense of smell" or "intuition" in general. It can refer literally to the ability to smell, like an animal’s keen sense of smell, or figuratively to a person’s intuition or instinct, especially for detecting something hidden or understanding a situation quickly.

## Why this project

It is a CLI based utility born because recently I've worked on many services with SDK exposure and with OpenAPI Spec.
This generate in me and idea for a tool capable of detech incosistency in you backend just looking at your specification.

It can be very usuful if you keep your spec.yml updated and in a good shape becuase can catch inconsistency or not tested situation. In a long term run it is easy to break compatibility or unwanted changes during input validation and responses.

It is in a early stage phase.

## Usage

```zsh
fiuto ./openapi.yml
```

This generates a result in json, maybe in future a UI can be built on top of this.

If you want override (or set server base url) you can call 
```zsh
fiuto --base-url 'http://127.0.0.1:8001' ./openapi.yml
```

In case something is not working with URL, the tool stops at the first request.

## Features

- [x] test every combination of input request
- [x] uses examples provided in the spec
- [x] json result easy to parse
- [x] support for full object example
- [x] support example for every propries
- [x] skip deprecated endpoints with `--skip-deprecated`
- [x] send request with a token using `--jwt <string>` (this allows to test endpoints behind an auth)

## Limitations

- just drilling endpoint with `content: application/json`
- the post request must have `requestBody` with `$ref`
- spec should contains `components` section with the struct in ref above
- in every `property` of the component's schema you need `example` to be filled (it uses that at the moment) or full example for the entire object

## Idea

- check responses
- support for headers to inject
- test combination of headers?
- test different input other then examples provided
- use the full example of the object instead examples for every field
- inject token for request with auth (CLI parameter)
- allow selection of server from `servers` property of spec with base url to use
- test `nullable` field
- support get with payload
- missing PUT
- options for waiting every x request, or between every request
- support POST with format like 
```
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
