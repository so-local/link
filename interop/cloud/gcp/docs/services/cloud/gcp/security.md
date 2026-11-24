# Google Cloud Credential Isolation

Every account operation uses that account's saved credential file.
The SDK's independent login cannot select a different principal for
boxes than the one used for storage.

## Credential Selection

Service-account keys and personal application-default credentials both
set `CLOUDSDK_AUTH_CREDENTIAL_FILE_OVERRIDE` from the account's
`GOOGLE_APPLICATION_CREDENTIALS`. This also applies to accounts saved
before the SDK override was recorded. Child calls discard ambient
`CLOUDSDK_*` settings before applying the account's environment. File-backed
account calls use a fresh private SDK configuration, removed when the client
ends. Saved token-file and impersonation settings in the user's independent
gcloud configuration cannot override the selected credential.

## Control Data

The saved file and its parent settings are trusted control data.
Replacing that file replaces the account's authority. File permissions
protect it from other local users, while the account owner remains
responsible for the permissions granted to the Google identity.
