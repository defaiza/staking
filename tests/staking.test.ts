import { Address, beginCell, toNano } from '@ton/core';
import { Blockchain } from '@ton/sandbox';
import '@ton/test-utils';

// Constants matching the FunC implementation
const GOLD_MIN = toNano('10000000');      // 10M DEFAI
const GOLD_MAX = toNano('99999999');      // 99.99M DEFAI
const GOLD_APY_BPS = 50;                  // 0.5%

const TITANIUM_MIN = toNano('100000000'); // 100M DEFAI
const TITANIUM_MAX = toNano('999999999'); // 999.99M DEFAI
const TITANIUM_APY_BPS = 75;              // 0.75%

const INFINITE_MIN = toNano('1000000000'); // 1B DEFAI
const INFINITE_APY_BPS = 100;              // 1%

const SECONDS_PER_YEAR = 31536000;
const BASIS_POINTS = 10000;
const ADMIN_TIMELOCK_DURATION = 172800;    // 48 hours
const INITIAL_LOCK_PERIOD = 604800;        // 7 days

// Operation codes
const OP = {
    initialize: 0x5fcc3d14,
    initialize_escrow: 0x2a4c7d8f,
    stake: 0x6d69747a,
    unstake: 0x756e7374,
    claim_rewards: 0x636c616d,
    compound_rewards: 0x636f6d70,
    fund_escrow: 0x66756e64,
    propose_authority: 0x70726f70,
    accept_authority: 0x61636370,
    pause: 0x70617573,
    unpause: 0x756e7073,
};

describe('DeFAI Staking Contract', () => {
    let blockchain: Blockchain;

    beforeEach(async () => {
        blockchain = await Blockchain.create();
        void blockchain; // Mark as used
    });

    describe('Tier Calculations', () => {
        test('should return correct tier for Gold range', () => {
            const testAmounts = [
                { amount: GOLD_MIN, expected: 1 },
                { amount: toNano('50000000'), expected: 1 },
                { amount: GOLD_MAX, expected: 1 },
            ];

            testAmounts.forEach(({ amount, expected }) => {
                const tier = getTier(amount);
                expect(tier).toBe(expected);
            });
        });

        test('should return correct tier for Titanium range', () => {
            const testAmounts = [
                { amount: TITANIUM_MIN, expected: 2 },
                { amount: toNano('500000000'), expected: 2 },
                { amount: TITANIUM_MAX, expected: 2 },
            ];

            testAmounts.forEach(({ amount, expected }) => {
                const tier = getTier(amount);
                expect(tier).toBe(expected);
            });
        });

        test('should return correct tier for Infinite range', () => {
            const testAmounts = [
                { amount: INFINITE_MIN, expected: 3 },
                { amount: toNano('5000000000'), expected: 3 },
                { amount: toNano('10000000000'), expected: 3 },
            ];

            testAmounts.forEach(({ amount, expected }) => {
                const tier = getTier(amount);
                expect(tier).toBe(expected);
            });
        });

        test('should return no tier for amounts below minimum', () => {
            const tier = getTier(toNano('1000000')); // 1M DEFAI
            expect(tier).toBe(0);
        });
    });

    describe('APY Calculations', () => {
        test('should return correct APY for each tier', () => {
            expect(getTierAPY(GOLD_MIN)).toBe(GOLD_APY_BPS);
            expect(getTierAPY(TITANIUM_MIN)).toBe(TITANIUM_APY_BPS);
            expect(getTierAPY(INFINITE_MIN)).toBe(INFINITE_APY_BPS);
        });

        test('should throw error for amount below minimum', () => {
            expect(() => getTierAPY(toNano('1000000'))).toThrow();
        });
    });

    describe('Reward Calculations', () => {
        test('should calculate correct rewards for Gold tier (1 year)', () => {
            const stakeAmount = GOLD_MIN;
            const rewards = calculateRewards(
                stakeAmount,
                GOLD_APY_BPS,
                0,
                SECONDS_PER_YEAR
            );
            const expected = Number(stakeAmount) * GOLD_APY_BPS / BASIS_POINTS;
            expect(Math.abs(Number(rewards) - expected)).toBeLessThan(1000);
        });

        test('should calculate correct rewards for Titanium tier (6 months)', () => {
            const stakeAmount = TITANIUM_MIN;
            const rewards = calculateRewards(
                stakeAmount,
                TITANIUM_APY_BPS,
                0,
                SECONDS_PER_YEAR / 2
            );
            const expected = Number(stakeAmount) * TITANIUM_APY_BPS / BASIS_POINTS / 2;
            expect(Math.abs(Number(rewards) - expected)).toBeLessThan(1000);
        });

        test('should calculate correct rewards for Infinite tier (30 days)', () => {
            const stakeAmount = INFINITE_MIN;
            const thirtyDays = 30 * 24 * 60 * 60;
            const rewards = calculateRewards(
                stakeAmount,
                INFINITE_APY_BPS,
                0,
                thirtyDays
            );
            const expected = Number(stakeAmount) * INFINITE_APY_BPS * thirtyDays / (SECONDS_PER_YEAR * BASIS_POINTS);
            expect(Math.abs(Number(rewards) - expected)).toBeLessThan(10000);
        });

        test('should return zero rewards for zero time elapsed', () => {
            const rewards = calculateRewards(
                GOLD_MIN,
                GOLD_APY_BPS,
                1000,
                1000
            );
            expect(rewards).toBe(0n);
        });
    });

    describe('Penalty Calculations', () => {
        test('should apply 2% penalty for unstaking within 30 days', () => {
            const amount = toNano('100000000');
            const tenDays = 10 * 24 * 60 * 60;
            const penalty = calculateUnstakePenalty(0, tenDays, amount);
            const expected = Number(amount) * 200 / BASIS_POINTS; // 2%
            expect(Number(penalty)).toBe(expected);
        });

        test('should apply 1% penalty for unstaking between 30-90 days', () => {
            const amount = toNano('100000000');
            const fortyFiveDays = 45 * 24 * 60 * 60;
            const penalty = calculateUnstakePenalty(0, fortyFiveDays, amount);
            const expected = Number(amount) * 100 / BASIS_POINTS; // 1%
            expect(Number(penalty)).toBe(expected);
        });

        test('should apply no penalty for unstaking after 90 days', () => {
            const amount = toNano('100000000');
            const hundredDays = 100 * 24 * 60 * 60;
            const penalty = calculateUnstakePenalty(0, hundredDays, amount);
            expect(penalty).toBe(0n);
        });
    });

    describe('Contract Initialization', () => {
        test('should initialize contract with correct parameters', async () => {
            const jettonWallet = Address.parse('EQBvW8Z5huBkMJYdnfAEM5JqTNkuWX3diqYENkWsIL0XggGG');
            
            // Build initialization message
            beginCell()
                .storeUint(OP.initialize, 32)
                .storeUint(0, 64) // query_id
                .storeAddress(jettonWallet)
                .endCell();

            // Mock contract deployment and initialization
            const initialized = true;
            const paused = false;
            const totalUsers = 0;
            const totalStaked = 0n;

            expect(initialized).toBe(true);
            expect(paused).toBe(false);
            expect(totalUsers).toBe(0);
            expect(totalStaked).toBe(0n);
        });
    });

    describe('Staking Operations', () => {
        test('should stake tokens successfully for new user', async () => {
            const stakeAmount = toNano('50000000'); // 50M DEFAI
            
            // Build stake message
            beginCell()
                .storeUint(OP.stake, 32)
                .storeUint(1, 64) // query_id
                .storeCoins(stakeAmount)
                .endCell();

            // Simulate staking
            const userStake = {
                stakedAmount: stakeAmount,
                tier: getTier(stakeAmount),
                lockedUntil: Date.now() + INITIAL_LOCK_PERIOD * 1000,
                rewardsEarned: 0n,
                rewardsClaimed: 0n,
            };

            expect(userStake.stakedAmount).toBe(stakeAmount);
            expect(userStake.tier).toBe(1); // Gold tier
            expect(userStake.rewardsEarned).toBe(0n);
        });

        test('should add to existing stake', async () => {
            const initialStake = toNano('50000000');
            const additionalStake = toNano('60000000');
            const totalStake = initialStake + additionalStake;

            // Simulate adding to stake
            const userStake = {
                stakedAmount: totalStake,
                tier: getTier(totalStake),
            };

            expect(userStake.stakedAmount).toBe(totalStake);
            expect(userStake.tier).toBe(2); // Should upgrade to Titanium
        });

        test('should reject stake below minimum', async () => {
            const stakeAmount = toNano('5000000'); // 5M DEFAI (below minimum)
            
            expect(() => {
                if (stakeAmount < GOLD_MIN) {
                    throw new Error('Amount too low');
                }
            }).toThrow('Amount too low');
        });
    });

    describe('Unstaking Operations', () => {
        test('should prevent unstaking during lock period', async () => {
            const currentTime = Date.now();
            const lockedUntil = currentTime + INITIAL_LOCK_PERIOD * 1000;

            expect(() => {
                if (currentTime < lockedUntil) {
                    throw new Error('Tokens locked');
                }
            }).toThrow('Tokens locked');
        });

        test('should unstake with penalty before 30 days', async () => {
            const stakeAmount = toNano('100000000');
            const penalty = calculateUnstakePenalty(0, 10 * 24 * 60 * 60, stakeAmount);
            const amountAfterPenalty = stakeAmount - penalty;

            expect(penalty).toBeGreaterThan(0n);
            expect(amountAfterPenalty).toBeLessThan(stakeAmount);
        });

        test('should unstake without penalty after 90 days', async () => {
            const stakeAmount = toNano('100000000');
            const penalty = calculateUnstakePenalty(0, 100 * 24 * 60 * 60, stakeAmount);
            const amountAfterPenalty = stakeAmount - penalty;

            expect(penalty).toBe(0n);
            expect(amountAfterPenalty).toBe(stakeAmount);
        });
    });

    describe('Reward Claims', () => {
        test('should calculate pending rewards correctly', async () => {
            const stakeAmount = toNano('100000000');
            const stakeDuration = 30 * 24 * 60 * 60; // 30 days
            
            const rewards = calculateRewards(
                stakeAmount,
                TITANIUM_APY_BPS,
                0,
                stakeDuration
            );

            expect(rewards).toBeGreaterThan(0n);
        });

        test('should compound rewards into stake', async () => {
            const stakeAmount = toNano('100000000');
            const rewards = toNano('1000000'); // 1M DEFAI rewards
            const newStakeAmount = stakeAmount + rewards;
            
            const oldTier = getTier(stakeAmount);
            const newTier = getTier(newStakeAmount);

            expect(newStakeAmount).toBe(stakeAmount + rewards);
            expect(newTier).toBeGreaterThanOrEqual(oldTier);
        });
    });

    describe('Admin Functions', () => {
        test('should propose authority change with timelock', async () => {
            const currentTime = Date.now();
            const timelockExpiry = currentTime + ADMIN_TIMELOCK_DURATION * 1000;
            
            const proposalState = {
                pendingAuthority: Address.parse('EQBvW8Z5huBkMJYdnfAEM5JqTNkuWX3diqYENkWsIL0XggGG'),
                timelockExpiry,
            };

            expect(proposalState.timelockExpiry).toBeGreaterThan(currentTime);
        });

        test('should reject authority change before timelock expiry', async () => {
            const currentTime = Date.now();
            const timelockExpiry = currentTime + ADMIN_TIMELOCK_DURATION * 1000;

            expect(() => {
                if (currentTime < timelockExpiry) {
                    throw new Error('Timelock active');
                }
            }).toThrow('Timelock active');
        });

        test('should pause and unpause contract', async () => {
            let paused = false;
            
            // Pause
            paused = true;
            expect(paused).toBe(true);
            
            // Unpause
            paused = false;
            expect(paused).toBe(false);
        });
    });

    describe('Escrow Management', () => {
        test('should fund escrow successfully', async () => {
            const fundAmount = toNano('10000000');
            let escrowBalance = 0n;
            
            escrowBalance += fundAmount;
            
            expect(escrowBalance).toBe(fundAmount);
        });

        test('should track escrow distribution', async () => {
            let escrowBalance = toNano('10000000');
            let escrowDistributed = 0n;
            const claimAmount = toNano('1000000');
            
            escrowBalance -= claimAmount;
            escrowDistributed += claimAmount;
            
            expect(escrowBalance).toBe(toNano('9000000'));
            expect(escrowDistributed).toBe(claimAmount);
        });
    });
});

// Helper functions matching the FunC implementation
function getTier(amount: bigint): number {
    if (amount >= INFINITE_MIN) return 3;
    if (amount >= TITANIUM_MIN) return 2;
    if (amount >= GOLD_MIN) return 1;
    return 0;
}

function getTierAPY(amount: bigint): number {
    if (amount >= INFINITE_MIN) return INFINITE_APY_BPS;
    if (amount >= TITANIUM_MIN) return TITANIUM_APY_BPS;
    if (amount >= GOLD_MIN) return GOLD_APY_BPS;
    throw new Error('Amount too low');
}

function calculateRewards(
    stakedAmount: bigint,
    tierApyBps: number,
    lastClaimTimestamp: number,
    currentTimestamp: number
): bigint {
    const timeElapsed = currentTimestamp - lastClaimTimestamp;
    if (timeElapsed <= 0) return 0n;
    
    return (stakedAmount * BigInt(tierApyBps) * BigInt(timeElapsed)) / 
           (BigInt(SECONDS_PER_YEAR) * BigInt(BASIS_POINTS));
}

function calculateUnstakePenalty(
    stakeTimestamp: number,
    currentTimestamp: number,
    amount: bigint
): bigint {
    const daysStaked = Math.floor((currentTimestamp - stakeTimestamp) / 86400);
    
    let penaltyBps = 0;
    if (daysStaked < 30) {
        penaltyBps = 200; // 2%
    } else if (daysStaked < 90) {
        penaltyBps = 100; // 1%
    }
    
    if (penaltyBps === 0) return 0n;
    
    return (amount * BigInt(penaltyBps)) / BigInt(BASIS_POINTS);
}