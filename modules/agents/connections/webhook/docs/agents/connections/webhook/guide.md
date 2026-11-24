# Webhook Module Guide

Configure the webhook validator before another application posts events to your agent. The bridge delivers a post only after its signature, size, and content type pass the checks below.

## Install the Validator

Follow the module's [installation instructions](README.md#add-the-webhook-module) before configuring the application's webhook sender.

## Before it's installed

Without it, your agent's bridge answers every post to its webhooks
`503`. Configure the sender to retry failed deliveries after installation.

## What it takes

A post is taken only when all of these hold, and every other post is
refused, [each with its own answer](security.md#a-post):

- It's [signed as the webhook's sender signs](security.md), with the webhook's secret,
  or with its previous one while it rolls: Link's scheme, or GitHub's
  for a webhook added `--sender github`.
- Its body is at most 1 MiB.
- It carries JSON (`application/json` or `application/*+json`,
  whose body is JSON) or text (`text/plain`, or any `text/` type).

### A post's id

A post it takes is handed back to your agent's bridge with its id: the
post's `link-id`, or for GitHub, which signs no id, its body's SHA-256.
The bridge refuses a post whose id it took before, and hands the rest
to your agent as a message from `webhook:<name>`.

### GitHub's content type

GitHub posts a form unless its webhook's content type is
`application/json`, and a form is refused: pick `application/json` when
you add the webhook there.
