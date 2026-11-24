# Credentials

> Credentials Prove Who May Do What

A credential is a secret that authorizes, named by whose it is, such as
this computer's credential for a box it reaches. This page builds on
[Secrets](../../../secret/docs/config/secrets/README.md) with what a
credential adds: who holds it, what it opens, and how it's issued,
rotated and revoked.

## List Your Credentials

Every credential is a secret, so [`lnk secret list`](../../../secret/contract/docs/config/secrets/more/contract.md#listed-for-lnk-secret-list) lists each one among
the secrets this machine holds. For each it says whose it is, where it's
kept and what it may do, never its value. [Held Credentials](held.md#where-theyre-kept)
says where each is kept on a machine and who there can read it.

```sh
lnk secret list
```

## What a Credential Adds to a Secret

A credential is kept, carried and guarded as every secret is, by the
rules in [How Local Link Handles Every Secret](../../../secret/docs/config/secrets/README.md#how-local-link-handles-every-secret).
A secret that authorizes needs more said of it, so each credential
Local Link mints, holds or checks is written down here, with its owner
and what it opens. A credential Local Link handles that this page
doesn't name is a bug.

### Whose Each Credential Is

A credential is named by whose it is: a user credential, a machine
credential, an agent credential, or a service credential, a service's
own and shared by no user. Whose it is decides who may hold it and who
answers for its use.

#### Accounts Are Their IDs

An account's ID, `acct-` and 32 hex digits, never changes, and its
username can change at will. Whatever remembers who someone is keeps
the ID, or an email the account service has verified, never a username.

### Handed On Only for Its Purpose

A caller receives the smallest credential needed for its operation.
Long-lived signers and decryption secrets stay with their owner, and
callers receive narrow tokens or sealed transfers with a bounded
lifetime.

### What Travels Is Short and Narrow

A credential that lasts never leaves where it's kept. What crosses a
network is made for one audience, one purpose and minutes.

### Issued, Rotated and Revoked

A credential Local Link mints, such as an access token or an agent's
key, is issued by its owner for one audience and lasts minutes or until
its binding ends, so it rotates by being made again. A credential you
hold for another service, such as a model's key, is issued by that
service. Local Link keeps and uses it, and deleting it here never
revokes it there, so rotate it at its issuer when that's needed.

### Each Service's Own

Each credential owner documents its own issuance, storage, and revocation.
[Releases](../../../../../scripts/docs/more/developing/releases.md) explains
release signers and the verification boundary for installed programs.

## Who Proves Who They Are

Each kind of caller proves itself in its own way, and to its own
audience. [Held credentials](held.md) are separate. They are yours, for
other services (model keys, connection tokens, cloud accounts, bucket
keys), and Local Link keeps and uses them but never mints them.

### Person

An identity provider authenticates a person before issuing a credential.
The receiving service validates that credential's issuer, audience, lifetime,
and authority rather than trusting a display name supplied by the caller.

### Machine

Its [installation key](installation.md#the-installation-key), signing for [access
tokens](access-token.md), proves it to the account service, then to an audience that accepts those access tokens.

### Visitor to a Tunnel

A [visitor pass](relay.md#a-visitors-pass) proves the visitor's permitted
access to its issuing service. The provider must check the pass's audience
and lifetime rather than accepting it as a machine credential.

### Relay User, Agent, and Service

An [operator token](relay.md#an-operator-token) authorizes its holder only
at its issuing service. An agent proves itself to its bridges with its
[agent credential](agent.md#an-agents-key), and a service uses its own signer
under the receiving service's declared verification rules.

## Where Each Is Listed From

`lnk secret list` asks [`lnk uninstall --parts --json`](../../../../../docs/more/modules/guide.md#uninstall), where each of
Link's modules keeping something [says so by part](../../../../../docs/more/modules/contracts.md): each
part names the secrets it holds in its `credentials` list, as
`{"what", "of", "kept", "may"}`, never a value. So the list is what the modules installed here hold,
each saying its own, and none reads another's files.

### Accounts, Harness, and Cloud

The harness module lists each agent's binding credential while it is bound,
and the cloud module lists each cloud account's credentials. Each owner
reports only its own records rather than inspecting another owner's files.

### Box, Bucket, and Measure

The box module lists Link's SSH key for boxes and the box SSH secret
that signs its certificates; the storage module each
bucket's keys and encryption password; the measure module an export's
headers.

### Secret

The secret module lists each secret [`lnk secret`](../../../secret/docs/config/secrets/README.md#secrets) keeps, where (the
Keychain, the keeper or your file), and where it's granted: agents'
model and fallback keys and their connections' tokens among them.

### Harness From Outside Link

OpenClaw's, Hermes's, and DeepSeek Harness's adapters list what their
harness keeps in each agent's home: OpenClaw's gateway token, Hermes's
API server key and its Signal relay's secret, and DeepSeek Harness's
copy of the endpoint's key. An adapter finds the agents' homes for its harness
through the harness contract, and lists them as the
`agents` part's credentials; the folders are the harness module's.

## Credential Kinds

The credential catalogue gives every kind of secret a module lists a
stable identifier and a link to its owning explanation. Credential views
use these kinds while keeping their existing display labels, and the
inventory is generated from the same declaration as those runtime values.

### Secrets in the Catalogue That Authorize Nothing

Most kinds are credentials, but a few are secrets that authorize
nothing, listed so that every secret a module holds is findable. A
bucket's encryption password encrypts files, and this installation's
sealing key opens what was sealed to it. A secret you keep is a
credential only when its value authorizes something, and unreadable
secrets are a status, not a value.

### Every Kind

The inventory below is every kind, each linked to the section that says
what it is, where it's kept and what it may do.
[Webhook Secrets](webhook.md) explains how a receiver authenticates a
signed post and distinguishes its possible refusals.

<!-- inventory:credentials:start -->

- [A bucket's encryption password (`bucket-encryption-password`)](held.md#a-buckets-keys-and-encryption-password)
- [A bucket's keys (`bucket-keys`)](held.md#a-buckets-keys-and-encryption-password)
- [A cloud account's key (`cloud-account-key`)](held.md#a-cloud-accounts-keys)
- [A connection's token (`connection-token`)](held.md#a-connections-token)
- [A device code (`device-code`)](installation.md#the-device-code)
- [A harness's token (`harness-token`)](held.md#a-harnesss-token)
- [A memory embedding proof (`memory-embedding-proof`)](agent.md#a-memory-embedding-proof)
- [A model's key (`model-key`)](held.md#a-models-key)
- [A remote tool's headers (`remote-tool-headers`)](held.md#a-tools-key-and-a-remote-tools-headers)
- [A sandbox proxy's secret (`sandbox-proxy-secret`)](agent.md#the-sandbox-proxys-secret)
- [A secret you keep (`kept-secret`)](held.md#a-secret-you-keep)
- [A service's signer (`service-signing-key`)](README.md#each-services-own)
- [A share link (`share-link`)](relay.md#a-share-link)
- [A storage token that refreshes itself (`storage-refresh-token`)](held.md#a-storage-token-that-refreshes-itself)
- [A tool's key (`tool-key`)](held.md#a-tools-key-and-a-remote-tools-headers)
- [A tunnel or share-link password (`tunnel-password`)](relay.md#passwords-for-tunnels-and-share-links)
- [A visitor's pass (`visitor-pass`)](relay.md#a-visitors-pass)
- [A webhook's secret (`webhook-secret`)](webhook.md)
- [An access token (`access-token`)](access-token.md)
- [An agent's key (`agent-key`)](agent.md#an-agents-key)
- [An export's header (`export-header`)](held.md#an-exports-headers)
- [An installation assertion (`installation-assertion`)](installation.md#the-installation-assertion)
- [An operator token (`operator-token`)](relay.md#an-operator-token)
- [Link's SSH key for boxes (`box-ssh-key`)](held.md#links-ssh-key-for-boxes)
- [Secrets you keep, unreadable (`unreadable-secrets`)](held.md#when-saved-secrets-cannot-be-read)
- [The box SSH secret (`box-ssh-secret`)](held.md#the-box-ssh-secret)
- [This installation's key (`installation-key`)](installation.md#the-installation-key)
- [This installation's sealing key (`installation-sealing-key`)](installation.md#the-sealing-key)

<!-- inventory:credentials:end -->

### Compatibility with Outside Modules

The `what` field remains a JSON string. Known labels resolve to their
catalogue kinds; older or outside labels remain readable and round-trip
unchanged. Link's own code constructs a credential view from an explicit
kind rather than inventing another label.

## What Each May Do

What each credential opens at the account service, the relay, another
service and an agent's bridges, where a dash means nothing there.

| Credential | Account service | Relay | A service | A bridge |
|---|---|---|---|---|
| Machine key | Access tokens, by assertion | — | — | — |
| Access token, `tunnel` | — | Open and serve tunnels | — | — |
| Access token, `share` | — | Share links | — | — |
| Access token, `visit` | — | Visit tunnels that let its account in | — | — |
| Access token, `api` | Its API, as the machine | — | — | — |
| Access token, `identify` | — | — | Say who it is, to its audience alone | — |
| Visitor's pass | — | Visit one tunnel | — | — |
| Operator token | — | Tunnels and share links, as its user | — | — |
| Agent key | — | — | — | Its agent's calls |

<!-- docs-tree:start -->

## Docs

- [Held Credentials](held.md): The credentials you hold for other services, such as a model provider's API key or a cloud account's keys, which Local Link keeps and uses but never mints
- [Agent Credentials](agent.md): What proves a call comes from an agent, and what keeps another program from acting as it: its key, its memory's embedding proof and its sandbox proxy's secret
- [Installation Credentials](installation.md): Installation credentials prove which installation is asking, and its sealing key, a secret kept beside them, opens what was sealed to it
- [Access Tokens](access-token.md): An access token identifies a principal and the authority it may exercise at one audience until an expiry time
- [Relay Credentials](relay.md): Relay credentials authorize a visitor, an operator, or whoever holds a link or a password at a tunnel or a share link
- [Webhook Secrets](webhook.md): What proves a post from outside comes from the sender that holds the secret, and the one way every receiver of Link's checks it

<!-- docs-tree:end -->
