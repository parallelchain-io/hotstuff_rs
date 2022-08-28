# HotStuff-rs Node Tree REST API Endpoints

## GET /node

### Query parameters

|Key |Value |Default value |Description |
|--- |---   |---         |--- |
|hash|`NodeHash` as `Base64Url` |N/A |Incompatible with `height`. Identifies the first Node in the chain returned by this endpoint. |
|height |number|N/A |Incompatible with `hash`. Identifies the first Node in the chain returned by this endpoint. |
|direction |"forward" or "backward" |"backward" |If "forward", the chain will extend forward in time, i.e. the first node will have the lowest block height. The converse if "backward". |
|limit |number|1 |*Maximum* length of the chain returned. Depending on the availability of Nodes, less than `limit` may be returned.|
|include\_not\_committed |"true" or "false" |"false" |Whether or not to include Nodes that have been inserted into the NodeTree but are not yet committed. If set to true, Nodes returned by this endpoint are not guaranteed to remain a part of the NodeTree. |

### Possible response status codes

|Status code |Interpretation |
|---         |---            |
|200         |               |
|400         |               |
|404         |               |

### Response body

`Vec<Node>`, serialized using the encoding specified in `crates::msg_types`.