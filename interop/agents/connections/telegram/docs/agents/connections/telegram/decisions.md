# Decisions

Why the Telegram module gives each agent a bot of its own.

## Why one Telegram bot per agent?

Telegram lets one program read a bot's messages, so two agents on one
bot would take turns missing them. `connect` refuses a bot another agent
here has, or one Telegram says another program reads.
