#!/usr/bin/env python3
"""hb_compare.py — compares LordOS shaping (build/shape-test, otf.c + shape.c +
shape_indic.c, shape_khmer.c, shape_myanmar.c, shape_use.c) with HarfBuzz, glyph by glyph and position by position.

    python3 hb_compare.py <fonts folder> [shape-test] [-v]

Needs `pip install uharfbuzz` and Debian's Noto fonts (fonts-noto-core,
fonts-noto-cjk) plus Inter (assets/fonts) copied into the folder. Expected result: all
identical (see docs/LANGUAGES.md)."""
import os, subprocess, sys
import uharfbuzz as hb

CASES = [('NotoSansArabic-Regular.ttf', 0, 'arab', ['مرحبا بالعالم', 'السلام عليكم', 'لا إله إلا الله', 'الإعدادات', 'ملفات', 'اللغة والمنطقة', 'بِسْمِ اللَّهِ الرَّحْمَٰنِ الرَّحِيمِ', 'كتب (مرحبا) [عالم]', 'ـــبـــ', 'فِي الْبَيْتِ', 'لأ لإ لآ', 'ئؤ ٱلْ', 'مُحَمَّدٌ', 'سَّ', 'ي\u200d']),
    ('NotoNaskhArabic-Regular.ttf', 0, 'arab', ['مرحبا بالعالم', 'السلام عليكم', 'لا إله إلا الله', 'بِسْمِ اللَّهِ الرَّحْمَٰنِ الرَّحِيمِ', 'فِي الْبَيْتِ', 'محمد', 'تحميل الملفات', 'كتب (مرحبا)', 'مُحَمَّدٌ']),
    ('NotoSansHebrew-Regular.ttf', 0, 'hebr', ['שלום עולם', 'הגדרות', 'בְּרֵאשִׁית בָּרָא אֱלֹהִים', 'שָׁלוֹם', 'עִבְרִית (מבחן)', 'וּ', 'שׁבּ']),
    ('NotoSans-Regular.ttf', 0, 'latn', ['Hello World', 'AVATAR Typography', 'office ﬁ fi ffl', 'café naïve', 'é', 'Привет мир', 'Ελληνικά']),
    ('NotoSansCJK-Regular.ttc', 0, 'hani', ['设置和语言', '中文输入法', '「引用」、。']),
    ('NotoSansCJK-Regular.ttc', 0, 'kana', ['こんにちは', 'カタカナ']),
    ('NotoSansCJK-Regular.ttc', 0, 'hang', ['안녕하세요', '한국어']),
    ('Inter-Regular.ttf', 0, 'latn', ['Hello World', 'AVATAR Typography', 'Ajustes del sistema', 'Wolf Tavern', 'café', 'office fi', 'T. V. A.', 'Fyodor']),
    ('NotoSansDevanagari-Regular.ttf', 0, 'deva', ['नमस्ते', 'हिन्दी', 'क्षत्रिय', 'प्रेम', 'धर्म', 'कार्य', 'राष्ट्र', 'श्री', 'कृपया', 'द्वार', 'किसी', 'स्त्री', 'पृथ्वी', 'अर्थ', 'विद्यार्थी', 'हिंदी भाषा', 'र्कि', 'क़िला', 'ज़्यादा', 'कर्ण', 'ट्रेन', 'शक्ति', 'सिद्धि', 'उन्होंने', 'आर्द्र', 'ह्म']),
    ('NotoSansBengali-Regular.ttf', 0, 'beng', ['বাংলা', 'আমার সোনার বাংলা', 'কর্ম', 'শ্রী', 'ক্ষমা', 'কোথায়', 'বৌদ্ধ', 'র\u200d্যাব', 'কি', 'স্কুল', 'প্রথম', 'মৌমাছি', 'কোর্ট']),
    ('NotoSansGurmukhi-Regular.ttf', 0, 'guru', ['ਪੰਜਾਬੀ', 'ਸਤਿ ਸ੍ਰੀ ਅਕਾਲ', 'ਪ੍ਰੇਮ', 'ਕਿਤਾਬ', 'ਸੱਚ']),
    ('NotoSansGujarati-Regular.ttf', 0, 'gujr', ['ગુજરાતી', 'કર્મ', 'પ્રેમ', 'શ્રી', 'કિતાબ', 'સ્ત્રી']),
    ('NotoSansOriya-Regular.ttf', 0, 'orya', ['ଓଡ଼ିଆ', 'କର୍ମ', 'ପ୍ରେମ', 'କି', 'ଶ୍ରୀ', 'ସ୍ତ୍ରୀ']),
    ('NotoSansTamil-Regular.ttf', 0, 'taml', ['தமிழ்', 'கொண்டு', 'ஸ்ரீ', 'க்ஷ', 'பொருள்', 'கை', 'கௌ', 'நன்றி']),
    ('NotoSansTelugu-Regular.ttf', 0, 'telu', ['తెలుగు', 'కర్మ', 'శ్రీ', 'ప్రేమ', 'క్ష', 'కి', 'స్త్రీ', 'ధన్యవాదాలు']),
    ('NotoSansKannada-Regular.ttf', 0, 'knda', ['ಕನ್ನಡ', 'ಕರ್ಮ', 'ಶ್ರೀ', 'ಪ್ರೇಮ', 'ಕ್ಷ', 'ಕಿ', 'ಕೊ', 'ಸ್ತ್ರೀ']),
    ('NotoSansMalayalam-Regular.ttf', 0, 'mlym', ['മലയാളം', 'ശ്രീ', 'പ്രേമം', 'ക്ഷ', 'കേരളം', 'കൊ', 'കൈ', 'സ്ത്രീ', 'ന്റെ']),
    ('NotoSansThai-Regular.ttf', 0, 'thai', ['สวัสดี', 'ภาษาไทย', 'น้ำ', 'กำลัง', 'ทำ', 'ผู้ใหญ่', 'ขอบคุณครับ']),
    ('NotoSansLao-Regular.ttf', 0, 'lao ', ['ສະບາຍດີ', 'ນ້ຳ', 'ພາສາລາວ']),
    ('NotoSansDevanagari-Regular.ttf', 0, 'deva', ['भारत एक विशाल देश है जिसकी संस्कृति प्राचीन है', 'क्\u200dष', 'क्\u200cष', 'र्\u200dय', 'स्वतंत्रता दिवस', 'ड्रॉप', 'ग्रीष्म', 'त्र्यम्बक', 'ॐ', 'कृष्ण', 'ऋषि', 'श्र्य', 'द्ध्र', 'र्क्ष्म्य']),
    ('NotoSansBengali-Regular.ttf', 0, 'beng', ['রবীন্দ্রনাথ ঠাকুর', 'জ্ঞান', 'ক্ষ্ম', 'র্ক', 'বাংলাদেশের জাতীয় সংগীত', 'য়ে', 'ড়ি', 'স্ত্র্য']),
    ('NotoSansTamil-Regular.ttf', 0, 'taml', ['யாதும் ஊரே யாவரும் கேளிர்', 'ஸ்ரீ ராமன்', 'க்ஷேத்ரம்']),
    ('NotoSansTelugu-Regular.ttf', 0, 'telu', ['ఆంధ్రప్రదేశ్', 'ర్\u200dక', 'స్త్ర్య', 'కార్యక్రమం']),
    ('NotoSansKannada-Regular.ttf', 0, 'knda', ['ಕರ್ನಾಟಕ ರಾಜ್ಯ', 'ರ್\u200dಕ', 'ಸ್ತ್ರ್ಯ', 'ಕ್ರೀಡೆ']),
    ('NotoSansMalayalam-Regular.ttf', 0, 'mlym', ['കേരളത്തിന്റെ തലസ്ഥാനം', 'ന്\u200d', 'ര്\u200d', 'ല്\u200d', 'ള്\u200d', 'ണ്\u200d', 'ക്ര', 'ത്ര്യ', 'ൎക']),
    ('NotoSansGurmukhi-Regular.ttf', 0, 'guru', ['ਗੁਰੂ ਗ੍ਰੰਥ ਸਾਹਿਬ', 'ਖ਼ਾਲਸਾ', 'ਪ੍ਰਿੰਸ']),
    ('NotoSansGujarati-Regular.ttf', 0, 'gujr', ['ગુજરાત રાજ્ય', 'ર્ક્ષ', 'દ્ર']),
    ('NotoSansOriya-Regular.ttf', 0, 'orya', ['ଓଡ଼ିଶା ରାଜ୍ୟ', 'କ୍ଷ', 'ର୍ଯ୍ୟ']),
    ('NotoSansThai-Regular.ttf', 0, 'thai', ['ประเทศไทยมีพระมหากษัตริย์', 'กิ่ง', 'ปั้น', 'ซ้ำ', 'น้ำใจ']),
    ('NotoNastaliqUrdu-Regular.ttf', 0, 'arab', ['اردو', 'پاکستان', 'السلام علیکم', 'میں ٹھیک ہوں', 'نستعلیق', 'شکریہ', 'خوش آمدید', 'کتاب', 'محبت']),
    # Khmer, Myanmar and the Universal Shaping Engine (Tibetan, Sinhala, Balinese, Javanese, Mongolian…)
    ('NotoSansKhmer-Regular.ttf', 0, 'khmr', ['ភាសាខ្មែរ', 'សួស្តី', 'ព្រះរាជាណាចក្រកម្ពុជា', 'ស្ត្រី', 'កើត', 'ខ្ញុំស្រលាញ់អ្នក', 'អរគុណ', 'ក្រុង', 'ប្រទេស', 'ា', 'កៅ', 'ឿ', 'ក៏', 'ហ្វ្រ', 'គ្រឿង', 'ស្រ្តី']),
    ('NotoSerifKhmer-Regular.ttf', 0, 'khmr', ['ភាសាខ្មែរ', 'ព្រះរាជាណាចក្រកម្ពុជា', 'ស្ត្រី', 'កើត', 'គ្រឿង']),
    ('NotoSansMyanmar-Regular.ttf', 0, 'mymr', ['မြန်မာ', 'မင်္ဂလာပါ', 'ကျေးဇူးတင်ပါတယ်', 'မြန်မာစာ', 'နေကောင်းလား', 'ကြွ', 'သင်္ချိုင်း', 'ါ', 'ကြော', 'ပြည်ထောင်စု', 'ဦး', 'ဤ', 'ၵ', 'ၶၢၼ်']),
    ('NotoSerifMyanmar-Regular.ttf', 0, 'mymr', ['မြန်မာ', 'မင်္ဂလာပါ', 'ကြော', 'သင်္ချိုင်း']),
    ('NotoSerifTibetan-Regular.ttf', 0, 'tibt', ['བོད་ཡིག', 'བཀྲ་ཤིས་བདེ་ལེགས།', 'སྐད་', 'རྒྱ་མཚོ', 'ཨོཾ་མ་ཎི་པདྨེ་ཧཱུྃ', 'བསྒྲུབས', 'ཀྵ', 'ཀཱི', 'ཀཱུ', 'ཀྲྀ', 'ཧཱུྃ', 'ཀཱུ', 'ཀཱི', 'ྐ']),
    ('NotoSansSinhala-Regular.ttf', 0, 'sinh', ['සිංහල', 'ආයුබෝවන්', 'ශ්\u200dරී ලංකාව', 'ක්\u200dෂ', 'ප්\u200dරශ්නය', 'කෝ', 'ස්තූතියි', 'ි', 'කෞ', 'ර්\u200dය', 'ක්\u200dය', 'ශ්\u200dරී']),
    ('NotoSansBalinese-Regular.ttf', 0, 'bali', ['ᬅᬓ᭄ᬱᬭᬩᬮᬶ', 'ᬩᬲᬩᬮᬶ', 'ᬓ᭄ᬭ', 'ᬓᭀ', 'ᬓ᭄ᬲᬾ']),
    ('NotoSansJavanese-Regular.ttf', 0, 'java', ['ꦲꦏ꧀ꦱꦫꦗꦮ', 'ꦱꦸꦒꦼꦁ', 'ꦥꦿꦤꦺꦴꦠꦺꦴ', 'ꦏꦿꦶ', 'ꦧꦺꦴ']),
    ('NotoSansMongolian-Regular.ttf', 0, 'mong', ['ᠮᠣᠩᠭᠣᠯ', 'ᠰᠠᠢᠨ ᠪᠠᠢᠨ\u180eᠠ ᠤᠤ', 'ᠮᠣᠩᠭᠣᠯ ᠪᠢᠴᠢᠭ']),
    ('NotoSansBuginese-Regular.ttf', 0, 'bugi', ['ᨅᨔ ᨕᨘᨁᨗ', 'ᨒᨚᨈᨑ']),
    ('NotoSansTaiTham-Regular.ttf', 0, 'lana', ['ᨽᩣᩈᩤᩃ᩶ᩣ᩠ᨶᨶᩣ', 'ᨠᩤ᩠ᨾ', 'ᨲ᩠ᩅ']),
    ('NotoSansChakma-Regular.ttf', 0, 'cakm', ['𑄌𑄋𑄴𑄟𑄳𑄦', '𑄇𑄨']),
    ('NotoSansBrahmi-Regular.ttf', 0, 'brah', ['𑀩𑁆𑀭𑀸𑀳𑁆𑀫𑀻', '𑀓𑁆𑀱']),
    ('NotoSansSundanese-Regular.ttf', 0, 'sund', ['ᮃᮊ᮪ᮞᮛ ᮞᮥᮔ᮪ᮓ', 'ᮘᮞ ᮞᮥᮔ᮪ᮓ']),
    ('NotoSansCham-Regular.ttf', 0, 'cham', ['ꨌꩌ', 'ꨀꨇꩉ ꨌꩌ']),
    ('NotoSansBatak-Regular.ttf', 0, 'batk', ['ᯘᯮᯒᯖ᯲ ᯅᯖᯂ᯲', 'ᯂᯪ']),
    ('NotoSansLepcha-Regular.ttf', 0, 'lepc', ['ᰛᰩᰵᰛᰧᰶᰵ', 'ᰕᰦ']),
    ('NotoSansLimbu-Regular.ttf', 0, 'limb', ['ᤕᤠᤰᤌᤢᤱ', 'ᤁᤡ']),
    ('NotoSansNKo-Regular.ttf', 0, 'nko ', ['ߒߞߏ', 'ߞߊ߬ ߛߓߍ']),
    ('NotoSansAdlam-Regular.ttf', 0, 'adlm', ['𞤀𞤣𞤤𞤢𞤥', '𞤆𞤵𞤤𞤢𞤪']),
    ('NotoSansTaiViet-Regular.ttf', 0, 'tavt', ['ꪼꪕꪒꪾ']),
    ('NotoSansNewTaiLue-Regular.ttf', 0, 'talu', ['ᦟᦲᧅᦺᦑᦟᦹᧉ']),
    ('NotoSansTaiLe-Regular.ttf', 0, 'tale', ['ᥖᥭᥰ ᥘᥫᥴ']),
    ('NotoSansKhmer-Regular.ttf', 0, 'khmr', ['ក\u200dា', 'ក\u200cា', 'ក͏ា', '្ក', 'ក្ក្ក្ក', 'ក្រ្ក', 'ក្ក្រ', 'ករ៉ា', 'ក៌ា', 'ក៍', 'ក់ៈ', '១២៣', 'ឥឡូវ', 'ៗ', '(ក)', 'ឳ', 'ក៊ី', 'ប៉ុ', 'ស៊ើប', 'ៀក', 'កេះ', 'ពេញ']),
    ('NotoSansMyanmar-Regular.ttf', 0, 'mymr', ['က︀', 'ကြ\u200b', 'င်္ဂ', 'ျ', 'ကျွ', 'ကျွန်ုပ်', 'ချော့', 'ကော့', 'ဿ', 'ကို', 'ကြို့', '၁၂၃', 'ၐ', 'ဍ္ဍ', 'မ္ဘာ', 'ၾ', 'ၶ', 'ꩠ']),
    ('NotoSerifTibetan-Regular.ttf', 0, 'tibt', ['ཀཱུ', 'ཀཱི', 'ཀཱེ', 'ཀཱྀ', 'ཀ༹', 'ཀྐྐྐ', 'ཨཾཱ', 'ཀྃ', '༄༅', 'ཀ༹', 'ཏྭ', 'ཀ྆']),
    ('NotoSansSinhala-Regular.ttf', 0, 'sinh', ['ක්\u200d', 'ක\u200dා', 'ක්\u200cෂ', 'කො', 'කෘ', 'ඛ්\u200dය', 'ොක', 'ක්ෂ', 'ද්\u200dව', 'ඤ', 'ෆ්\u200dර', 'ශ්\u200dරී ලංකා ප්\u200dරජාතාන්ත්\u200dරික සමාජවාදී ජනරජය']),
    ('NotoSansMongolian-Regular.ttf', 0, 'mong', ['ᠠ᠋ᠠ', 'ᠭ᠌ᠡ', 'ᠰᠠᠢᠨ\u202fᠠ', 'ᠮᠣᠩᠭᠣᠯ ᠤᠯᠤᠰ', 'ᠠ\u180eᠠ', 'ᠬᠠᠭᠠᠨ']),
    ('NotoSansBalinese-Regular.ttf', 0, 'bali', ['ᬓᬾᬓ', 'ᬓ᭄ᬓᬾ', 'ᬳᬶ', 'ᬮ᭄ᬯ', 'ᬓᬼ', 'ᬦ᭄ᬢ᭄ᬭ', 'ᬭ᭄ᬓ']),
    ('NotoSansJavanese-Regular.ttf', 0, 'java', ['ꦏ꧀ꦭ', 'ꦫ꧀ꦏ', 'ꦏꦿꦺ', 'ꦏꦾ', 'ꦏꦺꦴꦁ', 'ꦗꦮꦶ']),
    ('NotoSansNKo-Regular.ttf', 0, 'nko ', ['ߒ ߞߏ ߞߊ߲', 'ߓߊߘߊ߲']),
    ('NotoSansTaiTham-Regular.ttf', 0, 'lana', ['ᨻᩕ', 'ᩉᩖ᩠ᨾ', 'ᨠᩮᩢᩣ']),]


def harfbuzz(path, index, text):
    font = hb.Font(hb.Face(hb.Blob.from_file_path(path), index))
    buf = hb.Buffer()
    buf.add_str(text)
    buf.guess_segment_properties()
    hb.shape(font, buf, {})
    out, pen = [], 0
    for info, pos in zip(buf.glyph_infos, buf.glyph_positions):
        out.append(f"{info.codepoint}@{pen + pos.x_offset},{pos.y_offset}")
        pen += pos.x_advance
    return "|".join(out), buf.direction


def main():
    fonts = sys.argv[1]
    tool = sys.argv[2] if len(sys.argv) > 2 and sys.argv[2] != '-v' else os.path.join(os.path.dirname(__file__), '..', 'build', 'shape-test')
    verbose = '-v' in sys.argv
    total = bad = 0
    for font, index, script, texts in CASES:
        path = os.path.join(fonts, font)
        if not os.path.exists(path):
            print(f"({font} missing: skipped)")
            continue
        for t in texts:
            want, direction = harfbuzz(path, index, t)
            rtl = '1' if direction == 'rtl' else '0'
            got = subprocess.run([tool, path, 'ui', script, rtl, t, str(index)], capture_output=True, text=True).stdout.strip()
            total += 1
            if got != want:
                bad += 1
                print('DIFFERENT', font, t)
                if verbose:
                    print('  harfbuzz:', want)
                    print('  lordos  :', got)
    print(f"{total - bad}/{total} identical to HarfBuzz")
    sys.exit(1 if bad else 0)


if __name__ == '__main__':
    main()
