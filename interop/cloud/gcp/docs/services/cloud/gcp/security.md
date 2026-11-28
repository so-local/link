# Google Cloud Credential Isolation

Every account operation uses that account's saved credential file, or
the machine's own service account for an account connected with
`--machine`. The SDK's independent login cannot select a different
principal for boxes than the one used for storage.

## Credential Selection

Service-account keys and personal application-default credentials both
set `CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE` from the account's
`GOOGLE_APPLICATION_CREDENTIALS`. This also applies to accounts saved
before the SDK override was recorded. Child calls discard ambient
`CLOUDSDK_*` settings before applying the account's environment. File-backed
account calls use a fresh private SDK configuration, removed when the client
ends. Saved token-file and impersonation settings in the user's independent
gcloud configuration cannot override the selected credential.

## Machine Service Account

An account connected as the machine's service account has no file. Its
settings name the metadata server and the account's email, and its gcloud
calls also use a fresh private SDK configuration, so a person signed in
to gcloud on that machine never replaces the machine's account. rclone
runs with no `GOOGLE_APPLICATION_CREDENTIALS` and finds the same account
at the metadata server.

## Control Data

The saved file and its parent settings are trusted control data. For
an account connected with `--machine`, the authority is the machine's
attached service account, which anyone running code on that machine
can also use.
Replacing that file replaces the account's authority. File permissions
protect it from other local users, while the account owner remains
responsible for the permissions granted to the Google identity.
