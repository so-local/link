# Owner Questions

Your agent can stop and ask you something before it goes on. The question can ask for a choice, some text, or a
yes or no. The [bridge](../bridge.md) delivers each
question to your chats, and `lnk agent question list` reads and answers
questions from this machine's command line too.

## List and Answer Questions

The bridge delivers immutable questions to the initiating connection or the owner's direct chats, with CLI fallback. `lnk agent question list` lists pending requests. Add a response code to review one. `--reply TEXT` submits a targeted answer. `--json` shows the full owner form, including the current draft revision and resolution. Requesting tools receive typed results without response codes.

## Save and Submit a Draft

`lnk agent question list CODE --draft answer.json` saves a JSON set submission for this CLI delivery. The command sends nothing. Review the draft with `lnk agent question list CODE --json`, then use `--submit REVISION` to send exactly that saved revision. Each delivery's draft is independent. Another accepted response closes the request before a stale draft can submit.
