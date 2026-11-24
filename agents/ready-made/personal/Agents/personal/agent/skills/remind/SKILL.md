---
name: remind
description: Create a schedule for a one-off reminder or a recurring nudge.
---

# Remind

1. Get what to say and when. Ask for anything missing. Use their zone,
   from about-me.md; if it is unknown, ask and save it there.
2. Create it with `cron__create`: a five-field `cron`, the reminder in
   `prompt`, and `answer: here` for this conversation, or a connected
   service the owner selected. A schedule has an ID, not a per-call name
   or `to`. For a one-off, set `recurring: false` and pin the minute,
   hour, day of the month and month, after confirming the intended date
   and zone. Never guess an ambiguous DST time. A cron uses the
   scheduler's configured `zone`, not the time zone saved in
   `about-me.md`. Before creating one, establish that the effective
   scheduler zone matches the owner's zone.
   If that is unknown or different, ask the owner to align the agent's
   scheduler setup first. Do not claim a reminder exists before the tool
   returns its saved ID.
3. Confirm in one line: what, when and the zone, such as "Reminder set:
   call Ana, Fri 18:00 Europe/Lisbon."
4. List reminders with `cron__list` and cancel one with `cron__delete`
   when asked. The list is scoped to this conversation. Read back the ID
   before cancelling, then list again to confirm removal. Do not claim an
   owner-wide inventory from a conversation's list.
5. To change a reminder's time, identify and cancel its old saved ID,
   then create the replacement with the confirmed new time. List again
   to verify one active replacement. The native tool has no update flag.

Never put a secret in a message.
