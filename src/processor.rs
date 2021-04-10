use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program::invoke,
    program_error::ProgramError,
    program_pack::{IsInitialized, Pack},
    pubkey::Pubkey,
    rent::Rent,
    sysvar::Sysvar,
};

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
        }
    }

    pub fn process_init_escrow(
        accounts: &[AccountInfo],
        amount: u64,
        program_id: &Pubkey,
    ) -> ProgramResult {
        msg!("process_init_escrow");
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
        let result = invoke(
            &owner_change_ix,
            &[
                temp_token_account.clone(),
                initializer.clone(),
                token_program.clone(),
            ],
        )?;

        msg!("------------- rESULT ------------------");
        msg!("{:?}", result);
        Ok(())
    }
}

#[cfg(test)]
mod tests {

    use std::str::FromStr;

    //use super::*;
    use crate::processor::Processor;
    use crate::state::Escrow;
    use solana_program::{program_pack::Pack, pubkey::Pubkey, rent::Rent, sysvar};

    use solana_sdk::account::{
        create_account_for_test, create_is_signer_account_infos, Account as SolanaAccount,
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

    #[test]
    fn test_init_escrow() {
        // println!("-------------------------- test_init_escrow --------------------------");
        // 0. `[signer]` The account of the person initializing the escrow
        // 1. `[writable]` Temporary token account that should be created prior to this instruction and owned by the initializer
        // 2. `[]` The initializer's token account for the token they will receive should the trade go through
        // 3. `[writable]` The escrow account, it will hold all necessary info about the trade.
        // 4. `[]` The rent sysvar
        // 5. `[]` The token program

        let escrow_program_id =
            Pubkey::from_str(&"escrow1111111111111111111111111111111111111").unwrap();

        let escrow_pubkey = Pubkey::new_unique();

        let token_id = spl_token::id();
        let mut rent_sysvar = create_account_for_test(&Rent::default());

        let account_min_balance = Rent::default().minimum_balance(Escrow::get_packed_len());

        let account_len = Escrow::get_packed_len(); // Account::get_packed_len();
        let escrow_len = Escrow::get_packed_len();

        let mut initializer_account =
            SolanaAccount::new(account_min_balance, account_len, &token_id);
        let mut temp_token_account =
            SolanaAccount::new(account_min_balance, account_len, &token_id);
        let mut initializer_token_to_receive_account =
            SolanaAccount::new(account_min_balance, account_len, &token_id);
        let mut escrow_account =
            SolanaAccount::new(account_min_balance, escrow_len, &escrow_pubkey);
        let mut token_account = SolanaAccount::new(account_min_balance, account_len, &token_id);

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

        // println!("{:?}\n", accounts);
        //
        //let accounts = create_is_signer_account_infos(accounts);
        let result = Processor::process_init_escrow(&accounts, 123, &escrow_program_id);
        println!("Result: {:?}", result);
        // println!("-------------------------- END test_init_escrow --------------------------");
    }
}
