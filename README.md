# HotStuff-rs 
HotStuff-rs is a Rust Programming Language implementation of the HotStuff consensus protocol. It offers:
1. Guaranteed Safety and Liveness in the face of up to 1/3rd of Voting Power being Byzantine at any given moment,
2. A small API (`Executor`) for plugging in state machine-based applications like blockchains, and
3. Well-documented, 'obviously correct' source code.

## Protocol Description 

### Components

#### Actor Thread

#### Catch-up Thread

#### NodeTree 

### Actor Behavior

#### Leader

#### Replica

#### New-View

### Types

#### **NODE(hash, parent, command, justify)**
|Field |Type |Description |
|---   |---  |---         |
|hash |`NodeHash` | |
|parent |`NodeHash` | |
|command |`Vec<u8>` | |
|justify |`QC` | |

#### **QC(view_num, node, sigs)**
|Field |Type |Description |
|---   |---  |---         |
|view_num |`u64` | |
|node |`NodeHash` | |
|sigs |`Vec<(PublicAddress, Signature)>` | |

### Messages

#### **PROPOSE(view_num, node)**
|Field |Type |Description |
|---   |---  |---         |
|view_num |`u64` | |
|node |`Node` | |

#### **VOTE(view_num, node, sig)**
|Field |Type |Description |
|---   |---  |---         |
|view_num |`u64` | |
|node |`Node` | |
|sig |`(PublicAddress, Signature)` | |

#### **NEW-VIEW(view_num, high_qc)**
|Field |Type |Description |
|---   |---  |---         |
|view_num|`u64` | |
|high_qc |`QC` | |

#### **INFORM(nodes)**
|Field |Type |Description |
|---   |---  |---         |
|nodes |`Vec<Node>` | |

