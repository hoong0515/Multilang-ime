"""공개 평가 데이터 내려받기 -> data/public/raw/

출처 (평가 전용으로 사용. 재배포·학습 사용 전에는 각 라이선스를 확인할 것)
- Leipzig Corpora Collection (https://wortschatz.uni-leipzig.de): 뉴스·위키백과·웹 문장 1만 개씩
- Tatoeba (https://tatoeba.org): 회화체 예문, CC BY 2.0 FR
- NSMC 네이버 영화 리뷰 (https://github.com/e9t/nsmc): 구어체 한국어, CC0
"""
import sys
import urllib.request
from pathlib import Path

LEIPZIG = "https://downloads.wortschatz-leipzig.de/corpora/{}.tar.gz"
SOURCES = {
    "kor_news_2022_10K.tar.gz": LEIPZIG.format("kor_news_2022_10K"),
    "kor_wikipedia_2021_10K.tar.gz": LEIPZIG.format("kor_wikipedia_2021_10K"),
    "kor-kr_web_2015_10K.tar.gz": LEIPZIG.format("kor-kr_web_2015_10K"),
    "jpn_news_2020_10K.tar.gz": LEIPZIG.format("jpn_news_2020_10K"),
    "jpn_wikipedia_2021_10K.tar.gz": LEIPZIG.format("jpn_wikipedia_2021_10K"),
    "eng_news_2023_10K.tar.gz": LEIPZIG.format("eng_news_2023_10K"),
    "jpn_sentences.tsv.bz2": "https://downloads.tatoeba.org/exports/per_language/jpn/jpn_sentences.tsv.bz2",
    "kor_sentences.tsv.bz2": "https://downloads.tatoeba.org/exports/per_language/kor/kor_sentences.tsv.bz2",
    "nsmc_ratings_test.txt": "https://raw.githubusercontent.com/e9t/nsmc/master/ratings_test.txt",
}


def main(dst: str = "data/public/raw"):
    out = Path(dst)
    out.mkdir(parents=True, exist_ok=True)
    for name, url in SOURCES.items():
        p = out / name
        if p.exists():
            print("skip", name)
            continue
        req = urllib.request.Request(url, headers={"User-Agent": "multilang-ime-eval/0.1"})
        with urllib.request.urlopen(req, timeout=120) as r:
            p.write_bytes(r.read())
        print(f"{name}: {p.stat().st_size / 1e6:.1f} MB")


if __name__ == "__main__":
    main(*sys.argv[1:])
