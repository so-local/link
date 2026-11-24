# Environment Secrets

Give an API key to an environment you run without putting its value in the environment file.
This example creates a sealed Stripe secret, says yes to giving it to one environment file and removes it afterward.

<!-- quickstart:steps -->

## Prepare an Environment

Install the environment module. Run the remaining commands from a
folder of your own, such as a new `link-secret-example` folder.

```sh
lnk mod add env
```

Save this file there as `env.toml`. It names the secret the environment needs and contains no secret value.

<!-- quickstart-file: env.toml -->
```toml
schema = 1
name = "example"
version = 1

[secrets]
sealed = ["STRIPE_KEY"]
```

## Create the Secret

Enter your own Stripe key when prompted. The secret is kept here and may
be attached only to requests for `api.stripe.com`.

```sh
lnk secret create STRIPE_KEY --internet-host api.stripe.com
```

## Approve It for the Environment

An environment you run acts as you, so it's given a secret its file
asks for only once you've said yes. A run at a terminal asks you. Say
yes ahead instead, for the file in this folder:

```sh
lnk env approval add STRIPE_KEY --spec env.toml
```

The environment module keeps your yes in your Safe for this file's path,
its `name` and the secret's ID. A copy of the file elsewhere, a changed
`name` or a secret made again asks again, as
[Approvals](../../../../../compute/env/docs/services/environments/approval/approvals.md)
explains.

## Inspect the Approval

List what you've approved. Listing prints names and IDs, never secret
values.

```sh
lnk env approval list
```

## Remove the Example Secret

Delete the example secret when you finish, which takes its approvals
with it. A directly handed value or an already dispatched request can
outlive deletion; named routes recheck on the next call, as
[Secrets](README.md#take-a-secret-back) explains.

```sh
lnk secret delete STRIPE_KEY
```

<!-- quickstart:end -->
