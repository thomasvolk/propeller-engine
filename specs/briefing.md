
```
propeller project get
```

Returns the current and (if exists) the pending project as json into stdout.

```
{
    "current": {
       ...
    },
    "pending": {
       ...
    }
}

```

Like the status command, it will use the socket to get the information from the propeller daemon.
If the daemon is not running, it will return an error message.
