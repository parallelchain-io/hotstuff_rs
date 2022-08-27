# HotStuff-rs Node Tree REST API Endpoints

## GET /node

### Query parameters

|Key |Value |Default value |Description |
|--- |---   |---         |--- |
|hash|`NodeHash` as `Base64Url` |N/A | |
|num |number|N/A | |
|direction |"forward" or "backward" |"backward" | |
|limit |number|1 | |
|include\_not\_committed |"true" or "false" |"false" | |

### Possible response status codes

|Status code |Interpretation |
|---         |---            |
|200         |               |
|400         |               |
|404         |               |

### Response body
