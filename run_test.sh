./clean_block_files.sh

echo "---- running original c code tests"
cd original_C
make
./a.out
cd ..

echo "---- running manual translation tests"
cd manually_translated_C
cargo build > /dev/null 2>&1
./target/debug/manually_translated_C
cd ..

echo "---- running automatic translation tests"
cd automatically_translated_C
cargo build > /dev/null 2>&1
sleep 1
./target/debug/automatically_translated_C
cd ..

echo "---- Running block diff test"
./diff_block_files.sh