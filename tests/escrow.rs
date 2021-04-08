
use paulx_solana_escrow::{processor as p, state::Escrow};
use solana_program::{instruction::{AccountMeta, Instruction}, program_pack::Pack, pubkey::Pubkey, rent::Rent, sysvar};
use solana_program_test::{ProgramTest, processor};
use solana_sdk::{account::Account, signature::{Keypair, Signer}, transaction::Transaction};

#[tokio::test]
async fn test_success() {
    let program_id = Pubkey::new_unique(); //Pubkey::from_str(&"PaulxEscrow11111111111111111111111111111111").unwrap();
    let mut program_test = ProgramTest::new(
        "paulx_escrow_example",
        program_id,
        processor!(p::Processor::process),
    );

    // initializer
    let initializer_key = Keypair::new();
    let initializer_token_y_account = Pubkey::new_unique();
    program_test.add_account(
        initializer_token_y_account,
        Account {
            lamports: 5,
            owner: spl_token::id(), // Can only withdraw lamports from accounts owned by the program
            ..Account::default()
        },
    );


    // escrow_account
    let escrow_account_pubkey = Pubkey::new_unique();
    let escrow_len = Escrow::get_packed_len();
    let empty_escrow = Escrow {
        is_initialized: false,
        initializer_pubkey: Pubkey::new(&[1; 32]),
        temp_token_account_pubkey: Pubkey::new(&[2; 32]),
        initializer_token_to_receive_account_pubkey: Pubkey::new(&[3; 32]),
        expected_amount: 0,
    };
    let mut packed_escrow = vec![0; escrow_len];
    Escrow::pack(empty_escrow, &mut packed_escrow).unwrap();
    let min_escrow_rent = Rent::default().minimum_balance(escrow_len); // need to fix this
    let escrow_account = Account {
        lamports: min_escrow_rent,
        owner: program_id,
        data: packed_escrow,
        ..Account::default()
    };
    program_test.add_account( escrow_account_pubkey, escrow_account);

    // temp token account
    let temp_token_account_pubkey = Pubkey::new_unique();
    let token_account_len = spl_token::state::Account::get_packed_len();
    let token_account = spl_token::state::Account::default();
    let mut packed_token_account = vec![0; token_account_len];
    spl_token::state::Account::pack(token_account, &mut packed_token_account).unwrap();
    let min_token_account_rent = Rent::default().minimum_balance(token_account_len); // need to fix this
    let token_account = Account {
        lamports: min_token_account_rent,
        owner: spl_token::id(),
        data: packed_token_account,
        ..Account::default()
    };
    program_test.add_account(temp_token_account_pubkey, token_account);


    /*
       pub fn set_authority(
           token_program_id: &Pubkey, 
           owned_pubkey: &Pubkey, 
           new_authority_pubkey: Option<&Pubkey>, 
           authority_type: AuthorityType, 
           owner_pubkey: &Pubkey, 
           signer_pubkeys: &[&Pubkey]
       ) -> Result<Instruction, ProgramError>

       let owner_change_ix = spl_token::instruction::set_authority(
           token_program.key,
           temp_token_account.key,
           Some(&pda),
           spl_token::instruction::AuthorityType::AccountOwner,
           initializer.key,
           &[&initializer.key],
       )?;
       */




    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;
    println!("recent_blockhash: {:?}", recent_blockhash);

    let mut data = [0u8; 9];
    hex::decode_to_slice("007b00000000000000", &mut data as &mut [u8]).unwrap();

    // 0. `[signer]` The account of the person initializing the escrow
    // 1. `[writable]` Temporary token account that should be created prior to this instruction and owned by the initializer
    // 2. `[]` The initializer's token account for the token they will receive should the trade go through
    // 3. `[writable]` The escrow account, it will hold all necessary info about the trade.
    // 4. `[]` The rent sysvar
    // 5. `[]` The token program
    let mut transaction = Transaction::new_with_payer(
        &[Instruction::new_with_bytes(
            program_id,
            &data,
            vec![
            AccountMeta::new(initializer_key.pubkey(), true),
            AccountMeta::new(temp_token_account_pubkey, false), // temp token account
            AccountMeta::new(initializer_token_y_account, false), // initializer token account (for the receiving of the new token)
            AccountMeta::new(escrow_account_pubkey, false), // escrow account
            AccountMeta::new(sysvar::rent::id(), false), // rent sys var
            AccountMeta::new(spl_token::id(), false), // token program
            ],
        )],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer, &initializer_key], recent_blockhash);
    banks_client.process_transaction(transaction).await.unwrap();
}
