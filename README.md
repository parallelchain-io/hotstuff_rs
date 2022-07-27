# HotStuff-rs 
HotStuff-rs is a Rust Programming Language implementation of the HotStuff consensus protocol. It offers:
1. Guaranteed Safety and Liveness in the face of up to 1/3rd of Voting Power being Byzantine at any given moment,
2. A small API (`Executor`) for plugging in state machine-based applications like blockchains, and
3. Well-documented, 'obviously correct' source code.

## Overview

![A graphic depicting a Tree of square-shaped nodes. Nodes are colored depending on how many confirmations they have.](./readme_assets/HotStuff_rs%20NodeTree%20Overview.png "Node Tree semantics.")

`hotstuff_rs::HotStuff` works to build a 'NodeTree': a directed acyclic graph of Nodes. Node is a structure with a `command` field which applications are free to populate with arbitrary byte-arrays. In consensus algorithm literature, we typically talk of consensus algorithms as maintaining state machines that change their internal states in response to commands, hence, the choice of terminology.

HotStuff guarantees that committed Nodes are *immutable*. That is, they can never be *un*-committed. This guarantee enables applications to make hard-to-reverse actions with confidence. 

A Node becomes committed the instant its third confirmation is written into the NodeTree. A confirmation for a Node `A` is another Node `B` such that there is path between `B` to `A`.

The choice of third confirmation to define commitment--as opposed to first or second--is not arbitrary. HotStuff's safety and liveness properties actually hinge upon on this condition. If you really want to understand why this is the case, you should read the [paper](./readme_assets/HotStuff%20paper.pdf). To summarize:

1. Classic BFT consensus algorithms such as PBFT require only 2 confirmations for commitment, but this comes at the cost of having expensive leader-replacement protocols.
2. Tendermint require only 2 confirmations for commitment and has a simple leader-replacement protocol, but needs an explicit 'wait-for-N seconds' step to guarantee liveness.

HotStuff is the first consensus algorithm with a simple leader-replacement algorithm that does not have a 'wait-for-N seconds' step, and thus can make progress as fast as network latency allows. 

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

#### **(Public Address, Signature)**
|Field |Type |Descrption |

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

