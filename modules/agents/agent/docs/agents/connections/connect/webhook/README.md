# Webhooks

Use `lnk agent webhook` to let GitHub, a script, or another service post signed events to your agent. Each post reaches your agent as a message from someone who isn't
you, and nothing the agent says goes back.

```sh
lnk agent start
lnk mod add webhook-connection
lnk agent webhook create deploys
lnk agent webhook create repo --sender github
```

## Install Its Module

The [webhook
module](../../../../../../connections/webhook/docs/agents/connections/webhook/README.md),
`lnk-webhook-connection`, checks each post's signature, size, and whether it carries JSON or
text. Until you install the module, your agent's webhooks answer every post
`503`, so their senders try again.

```sh
lnk mod add webhook-connection
```

## Create a Webhook

`create` prints the webhook's address on this machine,
`http://127.0.0.1:<port>/h/<id>`, and its secret. The secret shows
only this once. Keep it where the sender signs with it, since Local Link never
shows it again. An agent has as many webhooks as you add, each with its
own secret, all at one port that Local Link chooses at the first and keeps.

```sh
lnk agent webhook create deploys
```

### Check What the Harness Needs

The agent's harness must say who each message is from and keep a
conversation's tools, as Link Harness does. `create` refuses a harness that
can't, since your agent would read the post as yours, or give the post every
tool the agent has. Your agent restarts if it runs, and from then on its
bridge takes the webhook's posts.

## Reach a Webhook from Outside

The address takes posts only from this machine. Local Link gives it no public
address, and `create` says so. A service outside reaches the webhook only
through an address you put in front of it, such as a reverse proxy on a server
of yours, keeping the path `/h/<id>`.

### Keep the Address Across Restarts

The port stays the same across restarts, so an address you put in front
of it keeps working.

## Sign a Post as Link Signs

A webhook created without `--sender` takes posts signed with Local Link's own
scheme. The scheme uses three headers, and an HMAC-SHA256 of the secret over the words
`link webhook`, the time, the ID, and the body, each after a NUL byte.

### `link-time`

`link-time` holds when the post was signed, in seconds since 1970. The webhook refuses a post signed more
than 5 minutes from now, either side.

### `link-id`

`link-id` holds the post's own ID, never used twice. The webhook refuses a post whose ID it took
in the last 30 days.

### `link-signature`

`link-signature` holds `v1=` and the HMAC in hex, made with the webhook's secret over the
words, the time, the ID, and the body. The webhook refuses a post without one.

### Sign and Send a Post

A sender signs each post before it sends it. It takes the time in Unix
seconds and a new random ID, such as 16 random bytes in hex. It joins
the words `link webhook`, the time, the ID and the body, each pair apart
by a zero byte, and makes the HMAC-SHA256 of that with the webhook's
secret, in hex. `openssl dgst -sha256 -hmac <secret>` makes one in a
shell. Then it posts the body with the three headers.

```sh
curl --data '{"ref":"main"}' --header "content-type: application/json" --header "link-time: <time>" --header "link-id: <id>" --header "link-signature: v1=<signature>" "<address>"
```

## Take GitHub's Posts

In the repository's settings on GitHub, under Webhooks, add a webhook with
the public address you put in front of the webhook as its payload URL, `application/json` as its
content type, and the secret. GitHub signs each post with
`x-hub-signature-256`.

```sh
lnk agent webhook create repo --sender github
```

### Handle a Delivery GitHub Sends Again

GitHub signs no delivery ID or time, so the webhook knows a post by its body.
The webhook refuses (`401`) a redelivery, in GitHub's list of deliveries, of a post your agent already
took, as it would a replay. It takes a delivery that wasn't
taken before (your agent couldn't be reached) when GitHub redelivers it.

## Read What Your Agent Sees

A post that's signed right reaches your agent as a message from
`webhook:<name>`, in a conversation of that name. The webhook answers `202`
once your agent's harness has taken the post, without waiting for the agent's answer.
The message is the body as text, and your agent's answer goes nowhere.
The poster is told only that the post was taken.

### Treat a Post as a Stranger's Message

Link Harness quotes each of the post's lines and tells the model who
sent them, so a post never reads as yours. The conversation has only the
tools your agent's specification gives strangers (`[tools] strangers`), none
unless the specification says, so a post can't make the agent act.

### Send JSON or Text

A post must carry JSON (`application/json`, or a type ending in
`+json`, whose body is JSON) or text (`text/plain`, or any `text/`
type).

### Read Why a Post Is Refused

Each refused post gets its own answer:

| Post | Answer |
|---|---|
| Body over 1 MiB | `413` |
| Not signed as its sender signs | `400` or `401` |
| Signed, but neither JSON nor text | `415` |
| Signed with another secret, or 5 minutes off | `401` |
| Id taken in the last 30 days (GitHub's: its body) | `401` |
| Address with no webhook | `404` |
| Any post while your agent can't be reached | `503`, and taken when it's sent again |

## Ask Yourself a Question

A program can ask you a yes or no question through a webhook, such as a
deploy waiting for your go. A post whose body is `{"question": "Deploy
to production now?"}` alone goes straight to your direct chats, never
waking your agent, and is answered `202` with the question's `id`,
`status` and `expires`. You answer on your chat with the question's code, as
your [agent's bridge](../../../more/interface/bridge.md#questions-on-your-chats)
says.

### Fetch the Answer

The program fetches the answer with a `GET` of
`/h/<hook id>/questions/<id>`, [signed as a post is](#sign-a-post-as-link-signs) over an empty body. The `status`
is `waiting`, `allowed`, `refused`, or `expired`. A webhook GitHub signs
takes no `GET`, since GitHub only posts.

### Write the Question

A question is plain text of at most 600 characters, with no link in it (`422`), so a
leaked secret can't send you somewhere. A link is a scheme (`https://`),
`www.`, or a name a chat links by itself, such as `evil.example`, in its
full-width forms too. So a question says "version 1.2" but never a
file's name with its extension. A webhook asks at most 20 questions a
day (`429`), or as many as `--questions-per-day` says when you add the webhook.

```sh
lnk agent webhook create deploys --questions-per-day 5
```

## Give It a New Secret

`rotate` prints a new secret once, and keeps taking the previous one for
24 hours while you give the sender the new one. The agent needs no
restart. `--done` drops the previous secret at once.

```sh
lnk agent webhook rotate deploys
lnk agent webhook rotate deploys --done
```

## List and Delete Them

`list` shows each webhook's name, who signs it, and its address, never a
secret. `delete` forgets the webhook's secrets, so the address takes no
more posts, and restarts your agent if it runs.

```sh
lnk agent webhook list
lnk agent webhook delete deploys
```

## Keep Webhooks Apart

[Security](../security.md#webhooks) shows how Local Link keeps webhooks apart from everything else. [Decisions](../decisions.md#webhooks) says why webhooks work this way.
