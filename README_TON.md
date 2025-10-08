# DeFAI Staking Contract - TON Blockchain Implementation

This is the TON blockchain (FunC) implementation of the DeFAI Staking contract, transpiled from the original Solana Rust implementation.

## Overview

The DeFAI Staking contract enables users to stake DEFAI tokens and earn rewards based on a tiered system. The contract has been adapted for TON's architecture while maintaining the core functionality and security features of the original Solana implementation.

## Contract Architecture

### Main Components

1. **defai_staking.fc** - Core staking contract with all business logic
2. **defai_jetton_handler.fc** - Jetton (TON's fungible token standard) integration layer
3. **deploy.fif** - Deployment script for the contract
4. **tests.fc** - Comprehensive test suite

## Features

### Staking Tiers

The contract implements three staking tiers with different APY rates:

- **Gold Tier**: 10M - 99.99M DEFAI (0.5% APY)
- **Titanium Tier**: 100M - 999.99M DEFAI (0.75% APY)
- **Infinite Tier**: 1B+ DEFAI (1% APY)

### Core Functionality

1. **Stake Tokens**: Users can stake DEFAI tokens to earn rewards
2. **Unstake Tokens**: Withdraw staked tokens with time-based penalties
3. **Claim Rewards**: Claim accumulated rewards from the escrow
4. **Compound Rewards**: Reinvest rewards to increase stake
5. **Fund Escrow**: Add tokens to the reward pool

### Admin Functions

- **Initialize**: Set up the contract with initial parameters
- **Propose Authority Change**: Propose new admin with 48-hour timelock
- **Accept Authority Change**: Execute pending authority change after timelock
- **Pause/Unpause**: Emergency pause functionality

### Security Features

- **Timelock**: 48-hour delay for critical admin actions
- **Lock Period**: 7-day initial lock on staked tokens
- **Penalty System**: Early unstaking penalties (2% < 30 days, 1% < 90 days)
- **Pause Mechanism**: Emergency pause for security incidents

## Technical Details

### Storage Layout

The contract uses an optimized storage structure:
- Main contract state (158 bits): initialized, paused, total users, total staked
- Authority addresses (512 bits): current and pending authority
- Escrow data (248 bits): balance and distribution tracking
- User stakes: Stored in a dictionary with comprehensive stake data

### Message Opcodes

Standard operations:
- `0x5fcc3d14` - Initialize
- `0x6d69747a` - Stake
- `0x756e7374` - Unstake
- `0x636c616d` - Claim rewards
- `0x636f6d70` - Compound rewards

Jetton operations:
- `0xf8a7ea5` - Jetton transfer
- `0x7362d09c` - Transfer notification
- `0x53544b45` - Stake notification

### Error Codes

- `101` - Not initialized
- `102` - Already initialized
- `103` - Unauthorized
- `104` - Amount too low
- `105` - Insufficient stake
- `106` - Tokens locked
- `107` - No rewards
- `108` - Program paused
- `109` - Insufficient escrow

## Deployment

### Prerequisites

1. TON development environment (func, fift)
2. DEFAI jetton contract deployed
3. TON wallet with funds for deployment

### Deployment Steps

1. Compile the FunC contracts:
```bash
func -o defai_staking.fif -SPA defai_staking.fc
func -o defai_jetton_handler.fif -SPA defai_jetton_handler.fc
```

2. Run the deployment script:
```bash
fift -s deploy.fif 0 <jetton-wallet-address> defai-staking
```

3. Send deployment transaction:
```bash
lite-client -C "sendfile defai-staking-deploy.boc"
```

4. Initialize the contract:
```bash
lite-client -C "sendfile defai-staking-init.boc"
```

## Testing

Run the test suite:
```bash
func -o tests.fif -SPA tests.fc
fift -s tests.fif
```

## Integration

### For Users

Users interact with the contract through jetton transfers with specific forward payloads:

1. **Staking**: Send DEFAI tokens with stake notification
2. **Unstaking**: Send unstake request message
3. **Claiming**: Send claim request message

### For Developers

The contract provides getter methods for querying:
- `get_contract_state()` - Overall contract status
- `get_user_stake_info(address)` - User's stake details
- `get_pending_rewards(address)` - Unclaimed rewards
- `get_escrow_info()` - Escrow balance and distribution

## Gas Optimization

The TON implementation includes several optimizations:
- Efficient storage packing
- Optimized arithmetic operations using `muldiv`
- Minimal message passing
- Dictionary-based user data storage

## Migration from Solana

Key differences from the Solana implementation:
1. **Account Model**: TON uses smart contracts instead of Solana's account model
2. **Token Standard**: Jettons instead of SPL tokens
3. **Message Passing**: Asynchronous message-based instead of CPI
4. **Storage**: Cell-based storage with dictionaries
5. **Time**: Unix timestamps instead of slot-based time

## Security Considerations

1. **Reentrancy**: Protected through TON's message ordering
2. **Integer Overflow**: Using safe arithmetic operations
3. **Access Control**: Role-based with address verification
4. **Timelock**: Enforced for critical operations
5. **Emergency Pause**: Available for security incidents

## Audit Recommendations

Based on the original Solana audit, the following have been addressed:
- Proper authority validation
- Escrow balance tracking
- Penalty calculations
- Reward distribution logic
- Emergency mechanisms

## License

This implementation maintains the same license as the original Solana contract.

## Support

For issues or questions about the TON implementation:
- Review the test suite for usage examples
- Check the original Solana implementation for business logic
- Consult TON documentation for platform-specific features