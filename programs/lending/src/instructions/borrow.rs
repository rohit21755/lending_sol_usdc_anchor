use anchor_lang::prelude::*;
use anchor_spl::{associated_token::AssociatedToken, token_interface::{self, Mint, TokenAccount, TokenInterface, TransferChecked}};
use pyth_solana_receiver_sdk::price_update::{ PriceUpdateV2, get_feed_id_from_hex};

use crate::{constants::{MAX_AGE, SOL_USD_FEED_ID, USDC_USD_FEED_ID}, state::*};
use std::f32::consts::E;
use crate::error::ErrorCode;
#[derive(Accounts)]
pub struct Borrow<'info> {
    #[account(mut)]
    pub signer: Signer<'info>,

    pub mint: InterfaceAccount<'info, Mint>,
    
    #[account(
        mut,
        seeds = [mint.key().as_ref()],
        bump,
    )]
    pub bank: Account<'info, Bank>,

    #[account(
        mut,
        seeds = [b"tresaury", mint.key().as_ref()],
        bump,
    )]
    pub bank_token_account: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut, 
        seeds = [signer.key().as_ref()],
        bump,
    )]  
    pub user_account: Account<'info, User>,
    #[account(
        init_if_needed,
        payer = signer,
        associated_token::mint = mint,
        associated_token::authority = signer,
        associated_token::token_program = token_program
    )]
    pub user_token_acount: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
    pub price_update: Account<'info, PriceUpdateV2>,
}
// 1. Check if user has enough collateral to borrow
// 2. Warn if borrowing beyond the safe amount but still allow if within the max borrowable amount
// 3. Make a CPI transfer from the bank's token account to the user's token account
// 4. Update the user's borrowed amount and total borrowed value
// 5. Update the bank's total borrows and total borrow shares

pub fn process_borrow(ctx: Context<Borrow>, amount: u64) -> Result<()> {
    let bank = &mut ctx.accounts.bank;
    let user = &mut ctx.accounts.user_account;

    let price_update = &mut ctx.accounts.price_update;
    let total_colletral: u64;

    match ctx.accounts.mint.to_account_info().key() {
        key if key == user.usdc_address => {
            let sol_feed_id = get_feed_id_from_hex(SOL_USD_FEED_ID)?;
            let sol_price = price_update.get_price_no_older_than(&Clock::get()?, MAX_AGE, &sol_feed_id)?;
            let new_value = calculate_accrued_interest(user.deposited_sol, bank.interest_rate, user.last_updated)?;
            total_colletral = sol_price.price  as u64 * new_value;
        },
        _ => {
            let usdc_feed_id = get_feed_id_from_hex(USDC_USD_FEED_ID)?;
            let usdc_price = price_update.get_price_no_older_than(&Clock::get()?, MAX_AGE, &usdc_feed_id)?;
            let new_value = calculate_accrued_interest(user.deposited_usdc, bank.interest_rate, user.last_updated)?;
            total_colletral = usdc_price.price as u64 * new_value;
        }
    }
    let borrowable_amount = total_colletral.checked_mul(bank.liquidation_threshold).unwrap();

    if borrowable_amount < amount {
        return Err(ErrorCode::OverBorrowable.into())
    }
    let transfer_cpi_accounts = TransferChecked {
        from: ctx.accounts.bank_token_account.to_account_info(),
        to: ctx.accounts.user_token_acount.to_account_info(),
        authority: ctx.accounts.bank_token_account.to_account_info(),
        mint: ctx.accounts.mint.to_account_info()
    };
    let cpi_program = ctx.accounts.token_program.to_account_info();
    let mint_key = ctx.accounts.mint.key();
    let signer_seeds: &[&[&[u8]]] = &[&[
        b"treasury",
        mint_key.as_ref(),
        &[ctx.bumps.bank_token_account],
    ]];

    let cpi_context = CpiContext::new(cpi_program, transfer_cpi_accounts).with_signer(signer_seeds);

    let decimals = ctx.accounts.mint.decimals;

    token_interface::transfer_checked(cpi_context, amount, decimals)?;

    if bank.total_borrowed == 0 {
        bank.total_borrowed = amount;
        bank.total_borrowed_shares = amount;
    }

    let borrow_ratio = amount.checked_div(bank.total_borrowed).unwrap();
    let user_shares = bank.total_borrowed_shares.checked_mul(borrow_ratio).unwrap();

    match ctx.accounts.mint.to_account_info().key() {
        key if key == user.usdc_address => {
            user.deposited_usdc += amount;
            user.deposited_usdc_shares += user_shares;
        },
        _ => {
            user.deposited_sol += amount;
            user.deposited_sol_shares += user_shares;
        } 
    }
    Ok(())
}

fn calculate_accrued_interest(deposited_value: u64, interest_rate: u64, last_updated: i64) -> Result<u64> {
    let current_time = Clock::get()?.unix_timestamp;
    let time_elapsed = current_time - last_updated;
    let new_value = (deposited_value as f64 * E.powf(interest_rate as f32 * time_elapsed as f32) as f64) as u64;
    Ok(new_value) 
    
}