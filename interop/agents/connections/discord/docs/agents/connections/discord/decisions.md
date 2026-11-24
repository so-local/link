# Decisions

Why the Discord module gives each agent a bot of its own, and how it
sees a code arrive.

## Why one Discord bot per agent?

Discord sends a bot's messages to every connection it has, so two agents
on one bot would both answer. `connect` refuses a bot another agent
here, or running on a box, has.

## How does Link see a Discord code arrive?

On Discord's Gateway, for as long as it waits, with only the direct
messages intent: a bot can't list its direct messages over HTTP. It
connects before showing the code, so the code can't arrive unseen.
