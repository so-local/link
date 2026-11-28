# Security

Local Link keeps your secrets and runs every program in a sandbox. The
secret module owns the secrets, and the sandbox module owns the sandbox.

<!-- docs-tree:start -->

## Docs

### Pages

- [Credentials](docs/config/credentials/README.md): A credential is a secret that authorizes, named by whose it is, such as this computer's credential for a box it reaches
  - [Held Credentials](docs/config/credentials/held.md): The credentials you hold for other services, such as a model provider's API key or a cloud account's keys, which Local Link keeps and uses but never mints
  - [Agent Credentials](docs/config/credentials/agent.md): What proves a call comes from an agent, and what keeps another program from acting as it: its key, its memory's embedding proof and its sandbox proxy's secret
  - [Installation Credentials](docs/config/credentials/installation.md): Installation credentials prove which installation is asking, and its sealing key, a secret kept beside them, opens what was sealed to it
  - [Access Tokens](docs/config/credentials/access-token.md): An access token identifies a principal and the authority it may exercise at one audience until an expiry time
  - [Relay Credentials](docs/config/credentials/relay.md): Relay credentials authorize a visitor, an operator, or whoever holds a link or a password at a tunnel or a share link
  - [Webhook Secrets](docs/config/credentials/webhook.md): What proves a post from outside comes from the sender that holds the secret, and the one way every receiver of Link's checks it

### Modules

- [Sandbox Module](sandbox/README.md): The sandbox module, `lnk-sandbox`: the one sandbox Link runs every harness in, and anything else
- [Secret Module](secret/README.md): The secret module, `lnk-secret`, keeps your secrets on this machine and gives them only where you grant them

<!-- docs-tree:end -->
