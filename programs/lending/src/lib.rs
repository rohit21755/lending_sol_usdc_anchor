use anchor_lang::prelude::*;
use instructions::*;

mod state;
mod instructions;

declare_id!("9fZnnXrSVLHG8Z7J2DKgxuStbJso1Fy5nZ13GapHwasj");

#[program]
pub mod lending {


    use super::*;
    pub fn init_bank(ctx: Context<InitBank>, liquidation_threshold: u64,max_ltv: u64) -> Result<()> {
        process_init_bank(ctx, liquidation_threshold, max_ltv)?;
        Ok(())
    }

    pub fn init_user(ctx: Context<InitUser>, usdc_address: Pubkey) -> Result<()> {
        process_init_user(ctx, usdc_address)?;
        Ok(())
    }

    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        process_deposit(ctx, amount)?;
        Ok(())
    }

}


