echo "comparing original with automatically_translated"

diff original_C/block0.txt automatically_translated_C/block0.txt 
diff original_C/block1.txt automatically_translated_C/block1.txt 
diff original_C/block2.txt automatically_translated_C/block2.txt 
diff original_C/block3.txt automatically_translated_C/block3.txt 
diff original_C/block4.txt automatically_translated_C/block4.txt 
diff original_C/block5.txt automatically_translated_C/block5.txt 

echo "comparing original with manually_translated"

diff original_C/block0.txt manually_translated_C/block0.txt 
diff original_C/block1.txt manually_translated_C/block1.txt 
diff original_C/block2.txt manually_translated_C/block2.txt 
diff original_C/block3.txt manually_translated_C/block3.txt 
diff original_C/block4.txt manually_translated_C/block4.txt 
diff original_C/block5.txt manually_translated_C/block5.txt 