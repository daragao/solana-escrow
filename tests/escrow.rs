
use paulx_solana_escrow::{processor as p, state::Escrow};
use solana_program::{instruction::{AccountMeta, Instruction}, program_pack::Pack, pubkey::Pubkey, rent::Rent, sysvar};
use solana_program_test::{ProgramTest, processor};
use solana_sdk::{account::Account, signature::{Keypair, Signer}, transaction::Transaction};

#[tokio::test]
#[cfg(feature = "test-bpf")]
async fn test_success() {
    // TODO packing escrow instruction
    // escrow instruction
    let mut data = [0u8; 9];
    hex::decode_to_slice("007b00000000000000", &mut data as &mut [u8]).unwrap();

    let program_id = Pubkey::new_unique();

    // token x
    let token_x = Keypair::new();

    // token minter
    let minter = Keypair::new();
    
    // 0. `[signer]` The account of the person initializing the escrow
    let initializer_key = Keypair::new();
    // 1. `[writable]` Temporary token account that should be created prior to this instruction and owned by the initializer
    let temp_x_token_account = Keypair::new();
    // 2. `[]` The initializer's token account for the token they will receive should the trade go through
    let initializer_y_token_account = Keypair::new();
    // 3. `[writable]` The escrow account, it will hold all necessary info about the trade.
    let escrow_account = Keypair::new();
    // 4. `[]` The rent sysvar
    // 5. `[]` The token program

    let mut program_test = ProgramTest::new(
        "paulx_solana_escrow",
        program_id,
        processor!(p::Processor::process),
    );

    // initializer account
    program_test.add_account(
        initializer_y_token_account.pubkey(), 
        Account {
            lamports: 5,
            owner: spl_token::id(), // Can only withdraw lamports from accounts owned by the program
            ..Account::default()
        },
    );

    // escrow account
    let empty_escrow = Escrow {
        is_initialized: false,
        initializer_pubkey: initializer_key.pubkey(),
        temp_token_account_pubkey: temp_x_token_account.pubkey(),
        initializer_token_to_receive_account_pubkey: initializer_y_token_account.pubkey(),
        expected_amount: 0,
    };
    let mut packed_escrow = vec![0; Escrow::get_packed_len()];
    Escrow::pack(empty_escrow, &mut packed_escrow).unwrap();
    program_test.add_account(
        escrow_account.pubkey(), 
        Account {
            lamports: Rent::default().minimum_balance(Escrow::get_packed_len()),
            owner: program_id, // Can only withdraw lamports from accounts owned by the program
            data: packed_escrow,
            ..Account::default()
        },
    );

    // create temp token solana account
    program_test.add_account(
        temp_x_token_account.pubkey(), 
        Account {
            lamports: Rent::default().minimum_balance(spl_token::state::Account::LEN),
            owner: spl_token::id(), 
            data: vec![0; spl_token::state::Account::LEN],
            ..Account::default()
        },
    );

    // create tokne x mint account
    program_test.add_account(
        token_x.pubkey(), 
        Account {
            lamports: Rent::default().minimum_balance(spl_token::state::Mint::LEN),
            owner: spl_token::id(), 
            data: vec![0; spl_token::state::Mint::LEN],
            ..Account::default()
        },
    );

    // Start program test -------------------------
    
    let (mut banks_client, payer, recent_blockhash) = program_test.start().await;
    println!("recent_blockhash: {:?}", recent_blockhash);

    println!("-------------------------- {} --------------------------", "Init  Token");
    let mut transaction = Transaction::new_with_payer(
        &[
            // init token
            spl_token::instruction::initialize_mint(
                &spl_token::id(),
                &token_x.pubkey(),
                &minter.pubkey(),
                None,
                0,
            )
            .unwrap(),
            // init temp token account
            spl_token::instruction::initialize_account(
                &spl_token::id(),
                &temp_x_token_account.pubkey(),
                &token_x.pubkey(),
                &initializer_key.pubkey(),
            )
            .unwrap(),
        ],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer], recent_blockhash);
    banks_client.process_transaction(transaction).await.unwrap();
    println!("-------------------------- {} --------------------------", "END");

    println!("---------------------------------------------------------");
    println!("Accounts:");
    println!("\tinitializer_key:             {}", initializer_key.pubkey());
    println!("\ttemp_x_token_account:        {}", temp_x_token_account.pubkey());
    println!("\tinitializer_y_token_account: {}", initializer_y_token_account.pubkey());
    println!("\tescrow_account:              {}", escrow_account.pubkey());
    println!("\tminter:                      {}", minter.pubkey());
    println!("Tokens:");
    println!("\ttoken_x:                     {}", token_x.pubkey());
    println!("---------------------------------------------------------");

    let mut transaction = Transaction::new_with_payer(
        &[
        // escrow call
        Instruction::new_with_bytes(
            program_id,
            &data,
            vec![
            AccountMeta::new(initializer_key.pubkey(), true),
            AccountMeta::new(temp_x_token_account.pubkey(), false),        // temp token account
            AccountMeta::new(initializer_y_token_account.pubkey(), false), // initializer token account (for the receiving of the new token)
            AccountMeta::new(escrow_account.pubkey(), false),              // escrow account
            AccountMeta::new(sysvar::rent::id(), false),                   // rent sys var
            AccountMeta::new(spl_token::id(), false),                      // token program
            ],
        )],
        Some(&payer.pubkey()),
    );
    transaction.sign(&[&payer, &initializer_key], recent_blockhash);
    banks_client.process_transaction(transaction).await.unwrap();
}
