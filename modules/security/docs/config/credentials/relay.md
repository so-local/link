# Relay Credentials

Relay credentials authorize a visitor, an operator, or whoever holds a link or a password at a tunnel or a share link. Each
optional service provider owns how it issues, stores and revokes them.

## A visitor's pass

A visitor pass authorizes one visitor for the service that issued it.
Its audience and lifetime must be checked on every use. It must not be
accepted as an account or machine credential.

## An operator token

An operator token authorizes its holder at one operator's service.
The issuing provider owns its scope, storage and revocation. Ordinary
status output never prints the token.

## Passwords for tunnels and share links

A password protects access to the resource named by its provider.
Send it only to that provider over a protected connection, never on a
command line or in logs. The provider must bound guesses and store a
verification value instead of the original password.

## A share link

A share link is a bearer credential for the files or resource it names.
Anyone who receives it may exercise its grant until the issuing provider
expires or revokes it. Keep it out of ordinary status output and logs.
