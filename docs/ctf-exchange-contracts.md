# CTF Exchange Smart Contracts Reference

> Generated via librarian agent research (2026-03-19)

## 1. Architecture Overview

The Polymarket CTF Exchange is a **hybrid-decentralized exchange protocol** that facilitates atomic swaps between Conditional Token Framework (CTF) ERC1155 assets and an ERC20 collateral asset (USDC.e). It combines offchain order matching with on-chain, non-custodial settlement.

**Source**: [Polymarket/ctf-exchange](https://github.com/Polymarket/ctf-exchange) (audited by ChainSecurity)

## 2. Contract Addresses (Polygon Mainnet)

### Core Contracts

| Contract | Address | Purpose |
|----------|---------|---------|
| **CTF Exchange** | `0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E` | Standard market order matching & settlement |
| **Neg Risk CTF Exchange** | `0xC5d563A36AE78145C45a50134d48A1215220f80a` | Multi-outcome market matching |
| **Neg Risk Adapter** | `0xd91E80cF2E7be2e162c6513ceD06f1dD0dA35296` | Converts No tokens between outcomes |
| **Conditional Tokens (CTF)** | `0x4D97DCd97eC945f40cF65F87097ACe5EA0476045` | ERC1155 -- split, merge, redeem |
| **USDC.e (Collateral)** | `0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174` | 6-decimal ERC20 collateral |

### Resolution Contracts

| Contract | Address | Purpose |
|----------|---------|---------|
| **UMA Adapter** | `0x6A9D222616C90FcA5754cd1333cFD9b7fb6a4F74` | Connects Polymarket to UMA Oracle |
| **UMA Optimistic Oracle** | `0xCB1822859cEF82Cd2Eb4E6276C7916e692995130` | Market resolution proposals/disputes |

### Wallet Factory Contracts

| Contract | Address |
|----------|---------|
| Gnosis Safe Factory | `0xaacfeea03eb1561c4e67d661e40682bd20e3541b` |
| Polymarket Proxy Factory | `0xaB45c5A4B0c941a2F231C04C3f49182e1A254052` |

## 3. Exchange Core Functions

```solidity
abstract contract BaseExchange is ERC1155Holder, ReentrancyGuard { }
```

| Function | Description |
|----------|-------------|
| `matchOrders(takerOrder, makerOrders, takerFillAmount, makerFillAmounts)` | Match taker against multiple maker orders (operator-only) |
| `fillOrder(order, takerFillAmount)` | Fill a single order |
| `fillOrders(orders, takerFillAmounts)` | Fill multiple orders in one tx |
| `cancelOrders(orders)` | Cancel one or more orders |

### Matching Scenarios

1. **Complementary (Buy vs Sell)** -- Direct asset transfers, no CTF operations
2. **Mint (Two Buys)** -- Exchange mints new outcome token pairs from collateral (when `priceYes + priceNo > 1`)
3. **Merge (Two Sells)** -- Exchange merges outcome tokens into collateral (when `priceYes + priceNo < 1`)

### Order Structure (EIP-712)

```solidity
bytes32 public immutable domainSeparator;

function hashOrder(Order memory order) public view returns (bytes32) {
    return _hashTypedDataV4(
        keccak256(
            abi.encode(
                ORDER_TYPEHASH,
                order.salt,
                order.maker,
                order.signer,
                // ... additional fields
            )
        )
    );
}
```

## 4. Outcome Token Redemption (`redeemPositions`)

### IConditionalTokens Interface

```solidity
/// @dev Redeems a CTF ERC1155 token for the underlying collateral
/// @param collateralToken  The address of the positions' backing collateral token
/// @param parentCollectionId  The ID of the outcome collections common to the position
/// @param conditionId  The ID of the condition to split on
/// @param indexSets  Index sets of the outcome collection to combine with the parent
function redeemPositions(
    IERC20 collateralToken,
    bytes32 parentCollectionId,
    bytes32 conditionId,
    uint256[] calldata indexSets
) external;
```

### Gnosis ConditionalTokens Implementation

```solidity
function redeemPositions(
    IERC20 collateralToken,
    bytes32 parentCollectionId,
    bytes32 conditionId,
    uint[] calldata indexSets
) external {
    uint den = payoutDenominator[conditionId];
    require(den > 0, "result for condition not received yet");

    uint outcomeSlotCount = payoutNumerators[conditionId].length;
    require(outcomeSlotCount > 0, "condition not prepared yet");

    uint totalPayout = 0;
    uint fullIndexSet = (1 << outcomeSlotCount) - 1;

    for (uint i = 0; i < indexSets.length; i++) {
        uint indexSet = indexSets[i];
        require(indexSet > 0 && indexSet < fullIndexSet, "got invalid index set");

        uint positionId = CTHelpers.getPositionId(collateralToken,
            CTHelpers.getCollectionId(parentCollectionId, conditionId, indexSet));

        uint payoutNumerator = 0;
        for (uint j = 0; j < outcomeSlotCount; j++) {
            if (indexSet & (1 << j) != 0) {
                payoutNumerator = payoutNumerator.add(payoutNumerators[conditionId][j]);
            }
        }

        uint payoutStake = balanceOf(msg.sender, positionId);
        if (payoutStake > 0) {
            totalPayout = totalPayout.add(payoutStake.mul(payoutNumerator).div(den));
            _burn(msg.sender, positionId, payoutStake);
        }
    }

    if (totalPayout > 0) {
        if (parentCollectionId == bytes32(0)) {
            require(collateralToken.transfer(msg.sender, totalPayout),
                "could not transfer payout to message sender");
        } else {
            _mint(msg.sender,
                CTHelpers.getPositionId(collateralToken, parentCollectionId),
                totalPayout, "");
        }
    }

    emit PayoutRedemption(msg.sender, collateralToken, parentCollectionId,
        conditionId, indexSets, totalPayout);
}
```

### Redemption Parameters for Polymarket

| Parameter | Value |
|-----------|-------|
| `collateralToken` | `0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174` (USDC.e) |
| `parentCollectionId` | `bytes32(0)` -- always zero for top-level positions |
| `conditionId` | Market-specific condition ID |
| `indexSets` | `[1, 2]` -- redeems both outcomes (only winning pays) |

### Payout Mechanics

| Outcome | Payout Vector | Redemption |
|---------|---------------|------------|
| Yes wins | `[1, 0]` | Yes = $1, No = $0 |
| No wins | `[0, 1]` | Yes = $0, No = $1 |

**Key behavior**: `redeemPositions()` burns your **entire token balance** for the condition -- there is no `amount` parameter.

## 5. Resolution Flow

1. Market reaches resolution date/condition
2. UMA Adapter oracle reports outcome via `reportPayouts()`
3. CTF contract records payout vector (`payoutDenominator` becomes non-zero)
4. Redemption becomes available for winning tokens

## 6. Gnosis Conditional Token Framework

### Source Repository

- **GitHub**: https://github.com/gnosis/conditional-tokens-contracts
- **Docs**: https://conditional-tokens.readthedocs.io/

### Core CTF Operations

| Function | Purpose |
|----------|---------|
| `prepareCondition(oracle, questionId, outcomeSlotCount)` | Initialize a condition's payout vector |
| `reportPayouts(questionId, payouts)` | Oracle reports results |
| `splitPosition(collateralToken, parentCollectionId, conditionId, partition, amount)` | Split collateral into outcome tokens |
| `mergePositions(collateralToken, parentCollectionId, conditionId, partition, amount)` | Merge outcome tokens back to collateral |
| `redeemPositions(collateralToken, parentCollectionId, conditionId, indexSets)` | Redeem winning tokens for collateral |

### Token ID Computation

```
Step 1: conditionId = keccak256(abi.encodePacked(oracle, questionId, outcomeSlotCount))
Step 2: collectionId = keccak256(abi.encodePacked(parentCollectionId, conditionId, indexSet))
Step 3: positionId = keccak256(abi.encodePacked(collateralToken, collectionId))
```

The `positionId` is the **ERC1155 token ID** for each outcome.

### Events

```solidity
event ConditionPreparation(bytes32 indexed conditionId, address indexed oracle,
    bytes32 indexed questionId, uint outcomeSlotCount);

event ConditionResolution(bytes32 indexed conditionId, address indexed oracle,
    bytes32 indexed questionId, uint outcomeSlotCount, uint[] payoutNumerators);

event PayoutRedemption(address indexed redeemer, IERC20 indexed collateralToken,
    bytes32 indexed parentCollectionId, bytes32 conditionId,
    uint[] indexSets, uint payout);
```

## 7. Rust Crates for CTF Interaction

### Official Polymarket Rust Client

| Field | Value |
|-------|-------|
| **Crate** | `polymarket-client-sdk` |
| **Version** | 0.4.4 (latest) |
| **Repository** | https://github.com/polymarket/rs-clob-client |

```toml
[dependencies]
polymarket-client-sdk = { version = "0.4", features = ["ctf", "clob", "data"] }
```

**Contract addresses built-in:**
```rust
use polymarket_client_sdk::{POLYGON, contract_config};

let config = contract_config(POLYGON, false).expect("polygon config");
// config.exchange:           0x4bFb41d5B3570DeFd03C39a9A4D8dE6Bd8B8982E
// config.collateral:         0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174
// config.conditional_tokens: 0x4D97DCd97eC945f40cF65F87097ACe5EA0476045
```

### Alternative Crates

| Crate | Version | Description |
|-------|---------|-------------|
| `polymarket` | 0.1.0 | Community SDK -- CLOB, on-chain ops, WebSocket |
| `polymarket-rs` | 0.2.0 | CLOB client with alloy primitives |
| `polymarket-rs-sdk` | 0.1.14 | REST APIs, WebSocket, order signing, Safe wallet |

## 8. Redemption Flow Summary

```
1. Check market resolution status (payoutDenominator[conditionId] > 0)
2. Call redeemPositions() on CTF contract (0x4D97DCd97eC945f40cF65F87097ACe5EA0476045)
   - collateralToken: 0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174
   - parentCollectionId: bytes32(0)
   - conditionId: <market condition ID>
   - indexSets: [1, 2]
3. Winning tokens are burned, USDC.e transferred to wallet
4. Losing tokens are burned with $0 payout
```
