# Installation Credentials

Installation credentials prove which installation is asking, and its sealing key, a secret kept beside them, opens what was sealed to it. Their provider
owns enrollment, storage and revocation, and shared callers receive only the
credential needed for one operation.

## The installation key

An installation signs assertions with its own signing secret. The public key can
be registered with an account provider. The private key stays on that
installation and is never copied into a box image.

## The sealing key

The sealing key is a secret that authorizes nothing: it only opens what
was sealed to this installation. The Secretive library owns it. A sender encrypts a transfer
for the receiver's public key and binds it to the named purpose and recipient.
The receiving process drops its key when it ends. A restartable hosted
transfer must enforce its own expiry and deletion rules.

## The installation assertion

Local Link signs a short-lived assertion for one audience.
The account provider checks its signature, time bounds and registered
installation before issuing an access token. An assertion is a credential and
must never appear in logs or ordinary command output.

## The device code

A device enrollment code lets an account provider associate a person's
approval with a waiting installation. Treat the code as a secret, limit its
lifetime, and refuse attempts after expiry or revocation.
