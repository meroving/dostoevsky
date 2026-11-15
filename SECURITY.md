# Security

Please don't report security problems in public issues.

Report them privately through GitHub: on the repository page, open **Security → Report a
vulnerability**. You'll get a reply within a week. Once a fix is out, we'll credit you in
the release notes unless you'd rather we didn't.

## What's in scope

- Anything that could lose or corrupt a writer's files, or write outside the file or
  folder they opened.
- Leaks of AI keys (they live in the macOS Keychain) or of document text to anyone other
  than the AI service the writer set up.
- The OpenRouter sign-in (a PKCE flow with a listener on 127.0.0.1).
- Parsing of untrusted `.md` files.

## Supported versions

Only the latest release gets fixes.
