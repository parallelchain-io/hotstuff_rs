# HotStuff-rs Participant Protocol Specification 

## 1. Types 

### **NODE(command, justify)**
|Field |Type |Description |
|---   |---  |---         |
|command |`Vec<u8>` | |
|justify |`QC` | |

### **NodeHash**
SHA256Hash over `parent ++ command ++ justify`.

### **QC(view_num, node_hash, sigs)**
|Field |Type |Description |
|---   |---  |---         |
|view_num |`u64` | |
|node_hash |`NodeHash` | |
|sigs |`Vec<(PublicAddress, Signature)>` | |

### **Sig(public_address, signature)**
|Field |Type |Description |
|---   |---  |---         |
|public_address |`SHA256Hash` | |
|signature |`Ed25519Signature`

## 2. Consensus API Messages

### **PROPOSE(view_number, node)**
|Field |Type |Description |
|---   |---  |---         |
|view_number |`u64` | |
|node |`Node` | |

### **VOTE(view_num, node_hash, sig)**
|Field |Type |Description |
|---   |---  |---         |
|view_num |`u64` | |
|node_hash |`NodeHash` | |
|sig |`Sig` | |

### **NEW-VIEW(view_num, high_qc)**
|Field |Type |Description |
|---   |---  |---         |
|view_num|`u64` | |
|high_qc |`QC` | |

## 3. NodeTree API Endpoints

## 4. Engine Sequence Flow 

### 4.1. Initialization

### 4.2. CatchUp

### 4.3. Progress

### 4.3.1. View Timeouts

View timeouts determine how long the Engine Thread remains on a View before deciding that further progress at the current View is unlikely and moving onto the next View.

If View timeout is a constant and Participants' View Numbers become unsynchronized at any point in the protocol's lifetime, then Participants will never become synchronized at the same View ever again, preventing further progress.

In order to ensure that Participants eventually synchronize on the same View Number for long enough to make progress, the duration of a View timeout increases exponentially every time a timeout is triggered until some high, configurable limit $L$: 

$Timeout(N, L) = min(2^{{4N/3}}, L)$ 

Where $L$ is the number of consecutive views that ended with a NEW-VIEW (without progress being made).
