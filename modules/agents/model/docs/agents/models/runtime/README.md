# Model Runtimes

`lnk model runtime` manages the local inference servers Local Link discovers. Add the server you are building at its own address so your agent can use it while you restart and develop it.

## Use a model at its own address

A model served on a port of its own is then a model like Ollama's.
[`lnk agent start --model.default <model>`](../../../../../agent/docs/agents/README.md#start-your-agent) uses it.

```sh
lnk model runtime add http://localhost:5000 --name mymodel
lnk model runtime remove mymodel
```

### Check What Your Server Answers

Your server needs to answer OpenAI's `/v1/models` and
`/v1/chat/completions`, as vLLM and llama.cpp do. Local Link adds only an
address on this machine, which is `localhost`, `127.0.0.1` or `[::1]`.
While nothing answers there, `list` shows it as not answering.

## List Runtime Servers

`lnk model runtime list` shows discovered and added servers, including added addresses that are not answering. `lnk model list` lists their model names. Use `lnk model runtime list` when you need a server's address.
