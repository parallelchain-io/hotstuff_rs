# HotStuff-rs Block Tree REST API Endpoints

### Reading this document
Quotes ("") are used in this document for clarity, but are not interpreted specially in query strings.

## GET /block

### Query parameters

|Key |Value |Default value |Description |
|--- |---   |---         |--- |
|hash|`BlockHash` as `Base64Url` |N/A |Incompatible with `height`. Identifies the first Block in the chain returned by this endpoint. |
|height |number|N/A |Incompatible with `hash`. Identifies the first Block in the chain returned by this endpoint. |
|direction |"forward" or "backward" |"backward" |If "forward", the chain will extend forward in time, i.e. the first block will have the lowest block height. The converse if "backward". |
|limit |number|1 |*Maximum* length of the chain returned. Depending on the availability of Blocks, less than `limit` may be returned.|
|include\_not\_committed |"true" or "false" |"false" |Whether or not to include Blocks that have been inserted into the BlockTree but are not yet committed. If set to true, Blocks returned by this endpoint are not guaranteed to remain a part of the BlockTree. |

### Possible response status codes

|Status code |Interpretation |
|---         |---            |
|200         |OK.            |
|400         |Invalid query string.               |
|404         |The Block identified by `hash` or `height` is not in this Participant's local database. Or, if `include_not_committed` is "false", not committed yet. |

### Response body

`Vec<Block>`, serialized using the encoding specified in `crates::msg_types`.