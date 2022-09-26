# HotStuff-rs Block Tree REST API Endpoints

### Reading this document
Quotes ("") are used in this document for clarity, but are not interpreted specially in query strings.

## GET /block

### Query parameters

|Key |Value |Default value |Description |
|--- |---   |---           |---         |
|hash|`BlockHash` as `Base64Url` |n/a (mandatory) |Identifies the 'anchor' Block in the returned chain. |
|anchor |"head" or "tail" |"head" |Whether `hash` identifies the head (highest Block) of the returned chain, or the tail (lowest Block) of the returned chain. |
|limit |number|1 |*Maximum* length of the chain returned. Depending on the availability of Blocks, less than `limit` may be returned.|
|speculate |"true" or "false" |"false" |Whether or not to include Blocks that have been inserted into the BlockTree but are not yet committed. If set to true, Blocks returned by this endpoint are not guaranteed to remain a part of the BlockTree. |

### Response status codes

|Status code |Interpretation |
|---         |---            |
|200         |OK.            |
|400         |Invalid query string.  |
|404         |The Block identified by `hash` is not in this Participant's local database. Or, if `speculate` is "false", not committed yet. |

### Response body

A borsh-serialized `Vec<Block>`, lowest-height Block first.

## GET /storage

### Query parameters

|Key |Value |Default value |Description |
|--- |---   |---           |---         |
|key |`Base64URL` |n/a (mandatory) |Identifies the key in *committed* Storage whose value the response will return. |

### Response status codes

|Status code |Interpretation |
|---         |---            |
|200         |OK.            |
|400         |Invalid query string. |
|404         |The provided key is not associated with any value in Storage (not even an empty value). |

### Response body

A borsh-serialized `Vec<u8>`.
