# Images, Documents and Voice Notes

> Images, Documents and Voice Notes It Can Read

Link Harness reads the images, documents and voice notes a message
holds, from the [Agent Client Protocol (ACP)](https://agentclientprotocol.com),
or from any of Local Link's connections, and keeps each as a file in the
conversation's folder.

## Send Images and Documents

A prompt on [ACP](../more/acp/README.md) takes a message's images as image
blocks and its documents as embedded resources, each in base64, a
document named by the last part of its URI. Images are PNG, JPEG, GIF
or WebP, and documents PDF or text (plain, Markdown or CSV), at most 10
MiB each and 8 to a message; a photo needs no caption. Anything else,
or a part that isn't what its type says, is refused, saying why.

<!-- json: link_agent_state_protocol::acp::schema::v1::PromptRequest -->
```json
{"sessionId": "link:main", "prompt": [
  {"type": "text", "text": "What does this say?"},
  {"type": "image", "mimeType": "image/png", "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg=="}]}
```

### From Local Link's Connections

A photo or document sent on one of Local Link's connections (Telegram, Slack,
Discord, Signal) reaches the agent the same way. The connection downloads it
and sends it with the message's text, and names to the agent any it
left out, a video, and why.

### Turn a Kind Off

An agent's specification's `[connections] images`, `documents`, and `voice`, or a connection's
own, turn each kind off. What a kind doesn't take is named to the agent as
left out.

### Where Images and Documents Are Kept

Each image and document is a file of its own in the conversation's folder, `parts/`, which
its message names (`[image: photo.jpg]`, in `conversation.md` too).
Forking a conversation copies its parts, and `lnk agent chat
export --format` takes them along.

### How Images and Documents Reach the Model

An image or a PDF reaches a model on Anthropic, OpenAI or OpenRouter as
the provider's own image or document; a text document reaches every
model as its text, under its name. A model on your computer is told
each image or PDF it was sent and left out, since what Local Link knows of a
model doesn't say whether it reads them.

### Receive Images and Documents from a Tool

A tool's answer may hold images and documents too (MCP's `image`
content, or a resource's `blob`), and they reach the model the same
way: [Link
File](../../../../../../../tools/file-tool/docs/agents/tools/file/README.md)'s
`file__read` gives an image or a PDF of its files so.

### Limit How Much a Prompt Sends

A prompt sends at most 20 MiB of images and documents, the latest first, and names
those before. The window counts an image as 1,600 tokens and a PDF as a
token for each 3 of its bytes.

### Conversations Kept Elsewhere

A conversation kept off this machine (through the storage bridge) has no folder for them, so its messages say what was left out
instead.

## Send a Voice Note

A voice note is an audio block on ACP, in base64: Ogg, MP3, MP4 audio,
AAC, WAV or WebM, as Telegram, Slack, Discord and Signal send one.
Before the turn, Link Harness has the note heard by a transcription model,
and the note's words begin the message's text: `[voice note] buy milk on the
way home`.

### Choose the Transcription Model

The model that the specification's `[model.<provider>] transcribe` names, on the agent's own provider,
hears the note through OpenAI's `POST /v1/audio/transcriptions`, which a
runtime on your computer may serve too. Local Link's specification names
`gpt-4o-mini-transcribe` for OpenAI. For another provider, set one:

```toml
schema = 1
[model.local]
transcribe = "whisper"
```

### What Happens to the Audio

With none, or one that can't hear it, the voice note is named to the
agent as left out, and why. The audio is kept in `parts/` as any part
is. The model gets only the note's words, never the audio.

## Attachment Design

Attachments are kept as files beside the conversation events and checked
when written and read, for reasons that [Media Decisions](decisions.md) explains.

<!-- docs-tree:start -->

## Docs

- [Decisions](decisions.md): Why Link Harness reads images and documents the way it does

<!-- docs-tree:end -->
