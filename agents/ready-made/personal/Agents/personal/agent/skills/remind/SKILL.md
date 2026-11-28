---
name: remind
description: Create a schedule for a one-off reminder or a recurring nudge.
---

# Remind

1. Get what to say and when. Ask for anything missing. Use their zone,
   from about-me.md; if it is unknown, ask and save it there.
2. Choose where it reaches them. If they say, such as "here", use it.
   Otherwise, when the head of their message names a service, such as
   "on Telegram", `answer: here` is that chat. When it says "on Link",
   this is Link's own chat, which they may not see from their phone.
   When `answer` offers a connected service, ask which should get it and
   put its name in `answer`; when it offers none, use `here` and say it
   lands in this chat.
3. Create it with `cron__create`: a five-field `cron`, the reminder in
   `prompt`, and the `answer` you chose. A schedule has an ID, not a
   per-call name or `to`. For a one-off, set `recurring: false` and pin
   the minute, hour, day of the month and month, after confirming the
   intended date and zone. Never guess an ambiguous DST time. A cron
   uses the scheduler's configured `zone`, not the time zone saved in
   `about-me.md`. Before creating one, establish that the effective
   scheduler zone matches the owner's zone. If that is unknown or
   different, ask the owner to align the agent's scheduler setup first.
   Do not claim a reminder exists before the tool returns its saved ID.
4. Add one line to `reminders.md`: its ID, when it fires and the zone,
   what it says, its `answer`, and the chat it was set in (the service
   the head names, and the chat's name if they gave one). A schedule fires in
   a conversation of its own, whose list sees nothing made here, so this
   file is the one list of every reminder.
5. Confirm it with `cron__list`, which shows this chat's schedules, then
   say it in one line: what, when and the zone, such as "Reminder set:
   call Ana, Fri 18:00 Europe/Lisbon."
6. To list reminders, read `reminders.md`. A one-off reminder that has
   fired is gone: remove its line once its time has passed.
7. To cancel one, read back its ID, cancel it with `cron__delete`,
   remove its line, and confirm with `cron__list` that no active copy
   remains. Only the chat that set a schedule can cancel it: refused
   here, say which chat it was set in, and keep its line.
8. To change a reminder's time, cancel its old ID, create the
   replacement with the confirmed new time, change its line to the new
   ID and time, and confirm one active replacement with `cron__list`.
   The native tool has no update flag.

Never put a secret in a message.
