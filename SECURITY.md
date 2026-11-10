<picture>
  <source media="(prefers-color-scheme: dark)" srcset="branding/headers/security-dark.png">
  <img alt="Risquette: Security" src="branding/headers/security-light.png">
</picture>

Thank you for helping keep Risquette and the people who use it safe.

## Supported versions

Security fixes go into the newest stable release, which installs automatically through Risquette's updater. Older versions and betas don't get separate fixes: update to the newest release.

| Version | Supported |
| --- | --- |
| Newest stable release | Yes |
| Older releases and betas | No |

## Reporting a vulnerability

**Please don't report security problems in public issues, pull requests or on Discuit.**

Report them privately through GitHub instead: open the [Security tab](https://github.com/tyler-Github/risquette/security) and choose **Report a vulnerability**, or go straight to the [private report form](https://github.com/tyler-Github/risquette/security/advisories/new). Only the maintainers can see what you send.

It helps to include:

- what the problem is and what an attacker could do with it,
- the steps to reproduce it, ideally with the smallest example that shows it,
- the Risquette version (Settings → Updates) and your operating system,
- anything you've found about the cause or a fix.

## What happens next

Risquette is maintained by volunteers, so response times vary, but every report is read. You can expect us to:

1. confirm we've received the report,
2. look into it and keep you updated in the private advisory,
3. fix it in a new release, and
4. publish the advisory once the fix is available, crediting you if you'd like.

Please give us a reasonable amount of time to release a fix before sharing details publicly.

## Scope

In scope:

- the Risquette app on Windows and Linux,
- its installer, uninstaller and updater, including how updates are downloaded and verified,
- the update server and website at [risquette.xnoid.studio](https://risquette.xnoid.studio).

Out of scope:

- **Discuit itself.** Problems in the Discuit server or website (things that also happen on discuit.org) belong to the Discuit project: see [discuitnet/discuit](https://github.com/discuitnet/discuit).
- Problems that need someone to already control your computer or user account.
- Windows SmartScreen warnings about the installer, which appear because the installer isn't code-signed yet.

## Testing responsibly

Discuit instances, including discuit.org, are live services with real people on them. When looking for problems:

- use your own accounts, or a Discuit instance you run yourself,
- don't access, change or delete other people's data,
- don't degrade the service for others, for example with automated or high-volume requests.
