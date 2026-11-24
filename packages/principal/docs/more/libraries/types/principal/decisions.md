# Principal Decisions

Why a principal is a user or a service and nothing else.

## Kinds

Which kinds a principal has, and what is left out.

### Why only two kinds?

Every grant answers who may act, and two kinds cover everyone who
does: a person, or a program acting on its own. A closed list keeps
each check exhaustive, so a new kind can't slip past one written before
it existed.

### Why does a workload act as a service with an ID of its own?

The service a workload runs as is minted apart from the workload's own
ID, since one service may run several workloads. What a workload may do
is then held by its service's roles, and the workload's ID stays
what runs, never who is granted.

### Why isn't a machine a principal?

A machine is where things run, not who acts. Its signer signs for its
user, so its access token names the user as its subject and the machine
in a field of its own, and access follows the person rather than the
hardware.

### Why isn't someone a connection carries a principal?

A message's sender on a messaging service is named by the connection
and its own ID there, never as a user. No user is minted for a person
outside until something vouches for them, so a sender's name on another
service never becomes access here.
