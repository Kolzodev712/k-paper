# Security

k-paper is an unauthenticated laboratory exchange simulator. Do not expose it on a public network.

## Reporting a vulnerability

Use **GitHub private vulnerability reporting** for this repository:

https://github.com/Kolzodev712/k-paper/security/advisories/new

Do not publish credentials or exploit details in public issues.

When `PAPER_LAB_FAULTS=1`, `/test/*` routes can disconnect clients and drop acknowledgements. Keep fault routes off shared interfaces.

Maintainers triage reports as capacity allows; there is no guaranteed response SLA.

## Secrets

Rotate any credential that may have been used alongside this software in shared environments. Do not commit secrets to this repository.
