# Security policy

Waking processes untrusted work events and may reach powerful existing agent sessions. Report suspected routing bypass, signature verification failure, credential exposure, approval bypass, source isolation failure, or unsafe duplicate delivery privately using the repository's GitHub security reporting facility when it is enabled. Do not include live credentials or private task content in a public issue. If private reporting is unavailable, open a minimal issue requesting a private contact without exploit details or secrets.

The v0.1.0 preview has no promised security response SLA. Supported security fixes target the latest published version. There is no implied security audit.

## Relevant boundaries

- Only configured ingress verification and authenticated device delivery establish event provenance.
- Context similarity is not ownership or permission. Work continues only under trusted source/host/thread policy.
- A filesystem read-only sandbox does not constrain all remote tools. Inspect actual host permissions and keep independent signing authority outside the worker.
- Drafts bind exact content, destination, identity, and revisions. The agent cannot approve itself.
- A timeout after remote acceptance may be an unknown outcome. Do not erase this state to make retries appear successful.
- The relay sees webhook plaintext. TLS protects transport, not plaintext from the relay operator.
- Data directories and secret files must be private. Default logs and public diagnostics should not contain tokens, authorization headers, full comments, or code.

Release archive checksums detect mismatches against the published checksum file; they are not independent signatures. This project does not claim code signing, notarization, official plugin-directory approval, or real-provider E2E unless the release evidence explicitly records it.
