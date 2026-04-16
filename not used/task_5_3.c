#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    char author[100], title[150], publisher[100];
    int year, ld, lm, ly, rd, rm, ry, avail;
    char reader_name[50], reader_surname[50];
} Book;

// Radix Logic: Flatten date to YYYYMMDD
int get_date_val(Book *b) {
return b->ry * 10000 + b->rm * 100 + b->rd;
}

void append_to_json(Book *lib, int n) {
    FILE *f = fopen("data.json", "r+");
    f = fopen("data.json", "w");
    fprintf(f, "[]");
    fclose(f);
    f = fopen("data.json", "r+");
    fseek(f, -1, SEEK_END);
    while (fgetc(f) != ']') fseek(f, -2, SEEK_CUR);
    fseek(f, -1, SEEK_CUR);
    for (int i = 0; i < n; i++) {
        if (i != n-1){
            fprintf(f, "\n  { \"author\": \"%s\", \"title\": \"%s\", \"publisher\": \"%s\", \"year\": %d, \"return\": \"%02d/%02d/%d\", \"avail\": %d, \"reader\": \"%s %s\" },",
                    lib[i].author, lib[i].title, lib[i].publisher, lib[i].year,
                    lib[i].rd, lib[i].rm, lib[i].ry, lib[i].avail, lib[i].reader_name, lib[i].reader_surname);
        }
        else {
            fprintf(f, "\n  { \"author\": \"%s\", \"title\": \"%s\", \"publisher\": \"%s\", \"year\": %d, \"return\": \"%02d/%02d/%d\", \"avail\": %d, \"reader\": \"%s %s\" }",
                    lib[i].author, lib[i].title, lib[i].publisher, lib[i].year,
                    lib[i].rd, lib[i].rm, lib[i].ry, lib[i].avail, lib[i].reader_name, lib[i].reader_surname);
        }
    }
    fprintf(f, "\n]");
    fclose(f);
}

void radix_sort(Book *lib, int n) {
    int max = get_date_val(&lib[0]);
    for (int i = 1; i < n; i++) if (get_date_val(&lib[i]) > max) max = get_date_val(&lib[i]);
    for (int exp = 1; max / exp > 0; exp *= 10) {
        Book *out = malloc(n * sizeof(Book));
        int count[10] = {0};
        for (int i = 0; i < n; i++) count[(get_date_val(&lib[i]) / exp) % 10]++;
        for (int i = 8; i >= 0; i--) count[i] += count[i + 1];
        for (int i = n - 1; i >= 0; i--) {
            int digit = (get_date_val(&lib[i]) / exp) % 10;
            out[count[digit] - 1] = lib[i];
            count[digit]--;
        }
        memcpy(lib, out, n * sizeof(Book)); free(out);
    }
}

int main() {
    int n; printf("Enter book count: "); scanf("%d", &n);
    Book *lib = malloc(n * sizeof(Book));
    for (int i = 0; i < n; i++) {
        printf("\nBook %d Author: ", i + 1); scanf(" %[^\n]", lib[i].author);
        printf("\nTitle: ");
        scanf(" %[^\n]", lib[i].title);
        printf("\nPublisher: ");
        scanf(" %[^\n]", lib[i].publisher);
        printf("\nYear: ");
        scanf(" %d", &lib[i].year);
        printf("\nReturn Date (DD MM YYYY): ");
        scanf("%d %d %d", &lib[i].rd, &lib[i].rm, &lib[i].ry);
        printf("\nAvailable (1/0): ");
        scanf("%d", &lib[i].avail);
        if(!lib[i].avail) {
            printf("\nReader Name: "); scanf(" %[^\n]", lib[i].reader_name);
            printf("\nReader Surname: "); scanf(" %[^\n]", lib[i].reader_surname);
        } else { strcpy(lib[i].reader_name, "-"); strcpy(lib[i].reader_surname, "-"); }
    }
    radix_sort(lib, n);
    append_to_json(lib, n);
    printf("\n[SUCCESS] JSON Persistence Complete. Radix Sort applied.\n");
    free(lib);
    return 0;
}