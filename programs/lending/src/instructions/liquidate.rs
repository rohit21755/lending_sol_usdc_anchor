use anchor_lang::prelude::*;
use pyth_solana_receiver_sdk::price_update::{get_feed_id_from_hex, PriceUpdateV2};
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token_interface::{ self, Mint, TokenAccount, TokenInterface, TransferChecked };
use crate::constants::{MAX_AGE, SOL_USD_FEED_ID, USDC_USD_FEED_ID};
use crate::state::*;
use crate::error::ErrorCode;
use super::calculate_accrued_interest;
#[derive(Accounts)]
pub struct Liquidate<'info> {
    #[account(mut)]
    pub liquidator: Signer<'info>,
    pub price_update: Account<'info, PriceUpdateV2>,
    // we need mint account of the collatral and mint account of borrow
    // because we are going to perfrom two transactions
    // 1. liquidator paying the debt 
    // 2. liquidator receiving the collateral

    pub collateral_mint: InterfaceAccount<'info, Mint>,
    pub borrowed_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        seeds = [collateral_mint.key().as_ref()],
        bump,
    )]
    pub borrowed_bank: Account<'info, Bank>,
    #[account(
        mut,
        seeds = [borrowed_mint.key().as_ref()],
        bump,
    )]
    pub collateral_bank: Account<'info, Bank>,
    
    #[account(
        mut,
        seeds = [b"treasury", borrowed_mint.key().as_ref()],
        bump
    )]
    pub borrowed_bank_token_account: InterfaceAccount<'info, TokenAccount>,
    #[account(
        mut,
        seeds = [b"treasury", collateral_mint.key().as_ref()],
        bump
    )]
    pub collateral_bank_token_account: InterfaceAccount<'info, TokenAccount>,

    pub user_account: Account<'info, User>,

    #[account(
        init_if_needed,
        payer = liquidator,
        associated_token::mint = collateral_mint,
        associated_token::authority = liquidator,
        associated_token::token_program = token_program,
    )]
    pub liquidator_collateral_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        init_if_needed,
        payer = liquidator,
        associated_token::mint = borrowed_mint,
        associated_token::authority = liquidator,
        associated_token::token_program = token_program,
    )]
    pub liquidator_borrowed_token_account: InterfaceAccount<'info, TokenAccount>,
    pub system_program: Program<'info, System>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
}

pub fn process_liquidation(ctx: Context<Liquidate>) -> Result<()> {
    let collateral_bank = &mut ctx.accounts.collateral_bank;
    let borrow_bank = &mut ctx.accounts.borrowed_bank;
    let user = &mut ctx.accounts.user_account;

    let price_udpate = &mut ctx.accounts.price_update;

    let sol_feed_id = get_feed_id_from_hex(SOL_USD_FEED_ID)?;
    let usdc_feed_id = get_feed_id_from_hex(USDC_USD_FEED_ID)?;
    let sol_price = price_udpate.get_price_no_older_than(&Clock::get()?, MAX_AGE, &sol_feed_id)?;
    let usdc_price = price_udpate.get_price_no_older_than(&Clock::get()?, MAX_AGE, &usdc_feed_id)?;

    let total_collateral: u64;
    let total_borrowed: u64;

    match ctx.accounts.collateral_mint.to_account_info().key() {
        key if key == user.usdc_address => {
            let new_usdc = calculate_accrued_interest(user.deposited_usdc, collateral_bank.interest_rate, user.last_updated)?;
            total_collateral = usdc_price.price as u64 * new_usdc;
            let new_sol = calculate_accrued_interest(user.borrowed_sol, borrow_bank.interest_rate, user.last_updated_borrowed)?;
            total_borrowed = sol_price.price as u64 * new_sol;
        },
        _ => {
            let new_sol = calculate_accrued_interest(user.deposited_sol, collateral_bank.interest_rate, user.last_updated)?;
            total_collateral = sol_price.price as u64 * new_sol;
            let new_usdc = calculate_accrued_interest(user.borrowed_usdc, borrow_bank.interest_rate, user.last_updated_borrowed)?;
            total_borrowed = usdc_price.price as u64 * new_usdc;

        }
    }
    let health_factor = (total_collateral * collateral_bank.liquidation_threshold / total_borrowed) as f64;

    if health_factor >= 1.0 {
        return Err(ErrorCode::NotUnderCollateralized.into())
    }
     // liquidator pays back the borrowed amount back to the bank 
    let transfer_to_bank = TransferChecked {
        from: ctx.accounts.liquidator_borrowed_token_account.to_account_info(),
        mint: ctx.accounts.borrowed_mint.to_account_info(),
        to: ctx.accounts.borrowed_bank_token_account.to_account_info(),
        authority: ctx.accounts.liquidator.to_account_info(),
    };
    let cpi_program = ctx.accounts.token_program.to_account_info();
    let cpi_ctx_to_bank = CpiContext::new(cpi_program.clone(), transfer_to_bank);
    let decimals = ctx.accounts.borrowed_mint.decimals;
    let liquidation_amount = total_borrowed * collateral_bank.liquidation_close_factor;
    token_interface::transfer_checked(cpi_ctx_to_bank, liquidation_amount, decimals)?;
    
    // Transfer liquidation value and bonus to liquidator
    let liquidation_bonus = (liquidation_amount * collateral_bank.liqudation_bonus) + liquidation_amount;
    
    let transfer_to_liquidator = TransferChecked {
        from: ctx.accounts.collateral_bank_token_account.to_account_info(),
        mint: ctx.accounts.collateral_mint.to_account_info(),
        to: ctx.accounts.liquidator_collateral_token_account.to_account_info(),
        authority: ctx.accounts.collateral_bank_token_account.to_account_info(),
    };

    let mint_key = ctx.accounts.collateral_mint.key();
    let signer_seeds: &[&[&[u8]]] = &[
        &[
            b"treasury",
            mint_key.as_ref(),
            &[ctx.bumps.collateral_bank_token_account],
        ],
    ];
    let cpi_ctx_to_liquidator = CpiContext::new(cpi_program.clone(), transfer_to_liquidator).with_signer(signer_seeds);
    let collateral_decimals = ctx.accounts.collateral_mint.decimals;   
    token_interface::transfer_checked(cpi_ctx_to_liquidator, liquidation_bonus, collateral_decimals)?;
    Ok(())
}