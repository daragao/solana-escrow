use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack},
    pubkey::Pubkey,
    rent::Rent,
    sysvar::Sysvar,
};

use spl_token::state::Account as TokenAccount;

use crate::{error::EscrowError, instruction::EscrowInstruction, state::Escrow};

pub struct Processor;
impl Processor {
    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instruction_data: &[u8],
    ) -> ProgramResult {
        let instruction = EscrowInstruction::unpack(instruction_data)?;

        match instruction {
            EscrowInstruction::InitEscrow { amount } => {
                msg!("Instruction: InitEscrow");
                Self::process_init_escrow(accounts, amount, program_id)
            }
            EscrowInstruction::Exchange { amount } => {
                msg!("Instruction: Exchange");
                Self::process_exchange(accounts, amount, program_id)
            }
        }
    }

    pub fn process_init_escrow(
        accounts: &[AccountInfo],
        amount: u64,
        program_id: &Pubkey,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let initializer = next_account_info(account_info_iter)?;

        if !initializer.is_signer {
            msg!("initializer needs to be signer");
            return Err(ProgramError::MissingRequiredSignature);
        }

        // temp_token_account account will be owned by the program
        let temp_token_account = next_account_info(account_info_iter)?;

        let token_to_receive_account = next_account_info(account_info_iter)?;
        if *token_to_receive_account.owner != spl_token::id() {
            return Err(ProgramError::IncorrectProgramId);
        }

        let escrow_account = next_account_info(account_info_iter)?;
        // check if there is enough rent
        let rent = &Rent::from_account_info(next_account_info(account_info_iter)?)?;

        if !rent.is_exempt(escrow_account.lamports(), escrow_account.data_len()) {
            return Err(EscrowError::NotRentExempt.into());
        }

        // deserialize the data
        let mut escrow_info = Escrow::unpack_unchecked(&escrow_account.data.borrow())?;
        if escrow_info.is_initialized() {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        // write date to escrow state
        escrow_info.is_initialized = true;
        escrow_info.initializer_pubkey = *initializer.key;
        escrow_info.temp_token_account_pubkey = *temp_token_account.key;
        escrow_info.initializer_token_to_receive_account_pubkey = *token_to_receive_account.key;
        escrow_info.expected_amount = amount;

        // write date to escrow state/data account
        Escrow::pack(escrow_info, &mut escrow_account.data.borrow_mut())?;

        // PDA (Program Derived Address) with a static seed
        let (pda, _bump_seed) = Pubkey::find_program_address(&[b"escrow"], program_id);

        // CPI (Cross Program-Invocation)
        let token_program = next_account_info(account_info_iter)?;
        let owner_change_ix = spl_token::instruction::set_authority(
            token_program.key,
            temp_token_account.key,
            Some(&pda),
            spl_token::instruction::AuthorityType::AccountOwner,
            initializer.key,
            &[&initializer.key],
        )?;

        msg!("Calling the token program to transfer ownership...");
        invoke(
            &owner_change_ix,
            &[
                temp_token_account.clone(),
                initializer.clone(),
                token_program.clone(),
            ],
        )
    }

    pub fn process_exchange(
        accounts: &[AccountInfo],
        amount_expected_by_taker: u64,
        program_id: &Pubkey,
    ) -> ProgramResult {
        let account_info_iter = &mut accounts.iter();
        let taker = next_account_info(account_info_iter)?;

        if !taker.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        let takers_sending_token_account = next_account_info(account_info_iter)?;
        let takers_token_to_receive_account = next_account_info(account_info_iter)?;

        let pdas_temp_token_account = next_account_info(account_info_iter)?;
        let pdas_temp_token_account_info =
            TokenAccount::unpack(&pdas_temp_token_account.data.borrow())?;
        let (pda, bump_seed) = Pubkey::find_program_address(&[b"escrow"], program_id);

        if amount_expected_by_taker != pdas_temp_token_account_info.amount {
            msg!("error: amount_expected_by_taker != pdas_temp_token_account_info.amount");
            return Err(EscrowError::ExpectedAmountMismatch.into());
        }

        let initializers_main_account = next_account_info(account_info_iter)?;
        let initializers_token_to_receive_account = next_account_info(account_info_iter)?;
        let escrow_account = next_account_info(account_info_iter)?;

        let escrow_info = Escrow::unpack(&escrow_account.data.borrow())?;

        if escrow_info.temp_token_account_pubkey != *pdas_temp_token_account.key {
            msg!("error: escrow_info.temp_token_account_pubkey != *pdas_temp_token_account.key");
            return Err(ProgramError::InvalidAccountData);
        }
        if escrow_info.initializer_pubkey != *initializers_main_account.key {
            msg!("error: escrow_info.initializer_pubkey != *initializers_main_account.key");
            return Err(ProgramError::InvalidAccountData);
        }
        if escrow_info.initializer_token_to_receive_account_pubkey
            != *initializers_token_to_receive_account.key
        {
            msg!("error: escrow_info.initializer_token_to_receive_account_pubkey != *initializers_token_to_receive_account.key");
            return Err(ProgramError::InvalidAccountData);
        }

        let token_program = next_account_info(account_info_iter)?;

        let transfer_to_initializer_ix = spl_token::instruction::transfer(
            token_program.key,
            takers_sending_token_account.key,
            initializers_token_to_receive_account.key,
            taker.key,
            &[&taker.key],
            escrow_info.expected_amount,
        )?;
        msg!("Calling the token program to transfer tokens to the escrow's initializer...");
        invoke(
            &transfer_to_initializer_ix,
            &[
                takers_sending_token_account.clone(),
                initializers_token_to_receive_account.clone(),
                taker.clone(),
                token_program.clone(),
            ],
        )?;

        let pda_account = next_account_info(account_info_iter)?;
        let transfer_to_initializer_ix = spl_token::instruction::transfer(
            token_program.key,
            pdas_temp_token_account.key,
            takers_token_to_receive_account.key,
            &pda,
            &[&pda],
            pdas_temp_token_account_info.amount,
        )?;
        msg!("Calling the token program to transfer tokens to the taker...");
        invoke_signed(
            &transfer_to_initializer_ix,
            &[
                pdas_temp_token_account.clone(),
                takers_token_to_receive_account.clone(),
                pda_account.clone(),
                token_program.clone(),
            ],
            &[&[&b"escrow"[..], &[bump_seed]]],
        )?;

        let close_pdas_temp_acc_ix = spl_token::instruction::close_account(
            token_program.key,
            pdas_temp_token_account.key,
            initializers_main_account.key,
            &pda,
            &[&pda],
        )?;
        msg!("Calling the token program to close pda's temp account...");
        invoke_signed(
            &close_pdas_temp_acc_ix,
            &[
                pdas_temp_token_account.clone(),
                initializers_main_account.clone(),
                pda_account.clone(),
                token_program.clone(),
            ],
            &[&[&b"escrow"[..], &[bump_seed]]],
        )?;

        msg!("Closing the escrow account...");
        **initializers_main_account.lamports.borrow_mut() = initializers_main_account
            .lamports()
            .checked_add(escrow_account.lamports())
            .ok_or(EscrowError::AmountOverflow)?;
        **escrow_account.lamports.borrow_mut() = 0;
        Ok(())
        // XXX I am exhausted
    }
}

#[cfg(test)]
mod tests {

    use std::str::FromStr;

    use super::*;
    use solana_program::{
        instruction::Instruction, program_pack::Pack, program_stubs, rent::Rent, sysvar,
    };

    use solana_sdk::account::{
        create_account_for_test, create_is_signer_account_infos, Account as SolanaAccount,
        WritableAccount,
    };

    #[test]
    fn test_pack_unpack() {
        // my first Rust test... so proud!
        let check = Escrow {
            is_initialized: true,
            initializer_pubkey: Pubkey::new(&[1; 32]),
            temp_token_account_pubkey: Pubkey::new(&[2; 32]),
            initializer_token_to_receive_account_pubkey: Pubkey::new(&[3; 32]),
            expected_amount: 10,
        };
        assert!(check.is_initialized);

        let mut packed = vec![0; Escrow::get_packed_len()];

        let expected = vec![
            1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1,
            1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2,
            2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 3,
            3, 3, 3, 3, 3, 3, 3, 3, 3, 3, 10, 0, 0, 0, 0, 0, 0, 0,
        ];
        Escrow::pack(check, &mut packed).unwrap();
        assert_eq!(packed, expected);

        let unpacked = Escrow::unpack(&packed).unwrap();
        assert_eq!(unpacked, check);

        //println!("{:?}", check);
        // println!("{:?}", unpacked);
    }

    struct TestSyscallStubs {}
    impl program_stubs::SyscallStubs for TestSyscallStubs {
        fn sol_invoke_signed(
            &self,
            _instruction: &Instruction,
            _account_infos: &[AccountInfo],
            _signers_seeds: &[&[&[u8]]],
        ) -> ProgramResult {
            msg!("TestSyscallStubs::sol_invoke_signed()");

            // TODO
            // Stub behaviour of the invoke

            Ok(())
        }
    }

    fn test_syscall_stubs() {
        use std::sync::Once;
        static ONCE: Once = Once::new();

        ONCE.call_once(|| {
            program_stubs::set_syscall_stubs(Box::new(TestSyscallStubs {}));
        });
    }

    #[test]
    fn test_init_escrow() {
        // println!("-------------------------- test_init_escrow --------------------------");
        // 0. `[signer]` The account of the person initializing the escrow
        // 1. `[writable]` Temporary token account that should be created prior to this instruction and owned by the initializer
        // 2. `[]` The initializer's token account for the token they will receive should the trade go through
        // 3. `[writable]` The escrow account, it will hold all necessary info about the trade.
        // 4. `[]` The rent sysvar
        // 5. `[]` The token program
        test_syscall_stubs();

        let escrow_program_id =
            Pubkey::from_str(&"escrow1111111111111111111111111111111111111").unwrap();

        let escrow_pubkey = Pubkey::new_unique();

        let token_id = spl_token::id();
        let rent = Rent::default();
        let mut rent_sysvar = create_account_for_test(&rent);

        let escrow_len = Escrow::get_packed_len();
        let escrow_account_min_balance = rent.minimum_balance(escrow_len);

        let mut initializer_account = SolanaAccount::default();
        let mut temp_token_account = SolanaAccount::default();
        let mut initializer_token_to_receive_account = SolanaAccount::default();
        initializer_token_to_receive_account.set_owner(spl_token::id()); // set owner of initializer token account to spl_token
        let mut escrow_account =
            SolanaAccount::new(escrow_account_min_balance, escrow_len, &escrow_pubkey);
        let mut token_account = SolanaAccount::default();

        let mut accounts = [
            (&Pubkey::new_unique(), true, &mut initializer_account),
            (&Pubkey::new_unique(), true, &mut temp_token_account),
            (
                &Pubkey::new_unique(),
                true,
                &mut initializer_token_to_receive_account,
            ),
            (&Pubkey::new_unique(), true, &mut escrow_account),
            (&sysvar::rent::id(), true, &mut rent_sysvar),
            (&token_id, true, &mut token_account),
        ];

        let accounts = create_is_signer_account_infos(&mut accounts);

        Processor::process_init_escrow(&accounts, 123, &escrow_program_id)
            .expect("error: process_init_escrow()");
    }

    #[test]
    fn test_exchange() {
        // 0. `[signer]` The account of the person taking the trade
        // 1. `[writable]` The taker's token account for the token they send
        // 2. `[writable]` The taker's token account for the token they will receive should the trade go through
        // 3. `[writable]` The PDA's temp token account to get tokens from and eventually close
        // 4. `[writable]` The initializer's main account to send their rent fees to
        // 5. `[writable]` The initializer's token account that will receive tokens
        // 6. `[writable]` The escrow account holding the escrow info
        // 7. `[]` The token program
        // 8. `[]` The PDA account
        let escrow_program_id = "escrow1111111111111111111111111111111111111";
        let escrow_program_id = Pubkey::from_str(&escrow_program_id).unwrap();
        let initializer_pubkey = Pubkey::new_unique();
        let pdas_temp_token_pubkey = Pubkey::new_unique();
        let initializer_token_to_receive_account_pubkey = Pubkey::new_unique();

        let (pda, _bump_seed) = Pubkey::find_program_address(&[b"escrow"], &escrow_program_id); // temp_token_account owner pubkey

        let amount = 123;

        // setup escrow account
        let mut escrow_account = SolanaAccount {
            owner: pda,
            data: vec![0; Escrow::get_packed_len()],
            ..SolanaAccount::default()
        };
        Escrow {
            is_initialized: true,
            initializer_pubkey,
            temp_token_account_pubkey: pdas_temp_token_pubkey,
            initializer_token_to_receive_account_pubkey,
            expected_amount: amount,
        }
        .pack_into_slice(&mut escrow_account.data);

        // temp_token_account (account that ownership was set in  initialization)
        let mut pdas_temp_token_account = SolanaAccount {
            owner: pda,
            data: vec![0; TokenAccount::get_packed_len()],
            ..SolanaAccount::default()
        };
        TokenAccount {
            amount,
            state: spl_token::state::AccountState::Initialized,
            ..TokenAccount::default()
        }
        .pack_into_slice(&mut pdas_temp_token_account.data);

        let mut taker_account = SolanaAccount::default();
        let mut taker_token_send_account = SolanaAccount::default();
        let mut taker_token_receive_account = SolanaAccount::default();
        let mut initializer_account = SolanaAccount::default();
        let mut initializer_token_receive_account = SolanaAccount::default();

        let mut token_account = SolanaAccount::default();
        let mut pda_temp_account = SolanaAccount::default(); // temp_token_account owner

        let taker_pubkey = Pubkey::new_unique();
        let taker_token_send_pubkey = Pubkey::new_unique();
        let taker_token_receive_pubkey = Pubkey::new_unique();
        let escrow_pubkey = Pubkey::new_unique();
        let token_pubkey = Pubkey::new_unique();

        let accounts: [solana_program::account_info::AccountInfo; 9] = [
            (&taker_pubkey, true, &mut taker_account).into(),
            (
                &taker_token_send_pubkey,
                false,
                &mut taker_token_send_account,
            )
                .into(),
            (
                &taker_token_receive_pubkey,
                false,
                &mut taker_token_receive_account,
            )
                .into(),
            (&pdas_temp_token_pubkey, false, &mut pdas_temp_token_account).into(),
            (&initializer_pubkey, false, &mut initializer_account).into(),
            (
                &initializer_token_to_receive_account_pubkey,
                false,
                &mut initializer_token_receive_account,
            )
                .into(),
            (&escrow_pubkey, false, &mut escrow_account).into(),
            (&token_pubkey, false, &mut token_account).into(),
            (&pda, false, &mut pda_temp_account).into(),
        ];

        Processor::process_exchange(&accounts, amount, &escrow_program_id)
            .expect("error: process_exchange()");

        // TODO all the asserts
    }
}
