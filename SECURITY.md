# Security

## Reporting a vulnerability

Use this repository's **Security → Report a vulnerability** option when private
vulnerability reporting is enabled. If the option is unavailable, ask a repository
maintainer to arrange a private channel, without posting vulnerability details.
Do not assume an ordinary issue, pull request, fork or CI artifact is private.
The maintainers must configure a working private channel before public release;
this document does not claim that repository settings have already been enabled.

Include the affected commit/version, operating system, a minimal reproducer you
are allowed to share, expected and actual behavior, and the security impact.
Remove credentials, customer media and personal metadata. Prefer a small synthetic
input over a production project. Do not probe hosted services or access other
people's data to demonstrate a converter bug. Coordinate public disclosure and
any upstream report with the maintainers.

## Supported fixes

Security fixes are developed against `main`. Historical test builds do not have a
separate security-maintenance commitment. Include the exact build SHA in a report;
a version string alone may identify several internal test releases.

## Trust boundaries

Project and media files are untrusted input. Malformed input should produce an
error rather than a panic or an uncontrolled allocation. Parser limits are
defense in depth, not a process-wide memory, CPU, or decompression sandbox.
Gzip-bearing jobs and other compressed inputs must run with deployment-enforced
process isolation and resource limits appropriate to that deployment.

FX script evaluation uses an embedded JavaScript VM. Its loop, stack and recursion
limits are not a wall-clock, heap, filesystem, network, or authority sandbox.
Untrusted gzip or JavaScript jobs must run in a separate, unprivileged process or
stronger isolation boundary with externally enforced CPU, memory, wall-clock,
process-count, filesystem, and network policy. Give the worker only the specific
input/output access it needs and no production credentials. Reject or terminate a
job when its deployment limit is reached.

The repository does not prescribe universal quota values: safe bounds depend on
input policy, host capacity, and workload, and static limits can reject valid
large projects. Operators must choose, test, monitor, and enforce limits outside
the converter. These requirements describe the trust boundary; they do not claim
that the converter or embedded VM supplies a sandbox.

Optional native-format, Asset API and FX rendering integration helpers have
separate permissions and execution requirements. Do not run them on unreviewed
inputs with production credentials. Their existing publication hold remains in force.

## Before a public release

Review locked dependency advisories, third-party notices, fixture permissions and
embedded metadata. Audit the actual exported source and binary archives, not only
the containing private checkout. A repository visibility change also exposes old
history, releases and other repository content; a clean current tree is not a
history or credential audit. Record the checks and unresolved findings rather
than treating this checklist as a certification.
