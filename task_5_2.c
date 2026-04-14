#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef struct {
    char author[50];
    char title[100];
    char publisher[50];
    int year;
    char loan_date[15];
    char return_date[15];
    char availability[10];
    char reader_name[50];
    char reader_surname[50];
} BookRecord;
typedef struct {
    char publisher[50];
    int count;
} PubCount;
void create_and_populate_file(int n) {
    FILE *file = fopen("experiment.txt", "w");
    // Dynamic memory allocation and pointers
    BookRecord *records = (BookRecord *)malloc(n * sizeof(BookRecord));
    for (int i = 0; i < n; i++) {
        BookRecord *ptr = &records[i];
        printf("\n--- Record %d ---\n", i + 1);
        printf("Author: "); scanf(" %[^\n]", ptr->author);
        printf("Title: "); scanf(" %[^\n]", ptr->title);
        printf("Publisher: "); scanf(" %[^\n]", ptr->publisher);
        printf("Year of pub: "); scanf("%d", &ptr->year);
        printf("Loan Date (DD/MM/YYYY): "); scanf(" %[^\n]", ptr->loan_date);
        printf("Return Date (DD/MM/YYYY): "); scanf(" %[^\n]", ptr->return_date);
        printf("Availability (yes/no): "); scanf(" %[^\n]", ptr->availability);
        if (strcmp(ptr->availability, "no") == 0) {
            printf("Reader Name: "); scanf(" %[^\n]", ptr->reader_name);
            printf("Reader Surname: "); scanf(" %[^\n]", ptr->reader_surname);
        } else {
            strcpy(ptr->reader_name, "-");
            strcpy(ptr->reader_surname, "-");
        }
        // Storing formatted data with '|' separator
        fprintf(file, "%s|%s|%s|%d|%s|%s|%s|%s|%s\n",
            ptr->author, ptr->title, ptr->publisher, ptr->year,
            ptr->loan_date, ptr->return_date, ptr->availability,
            ptr->reader_name, ptr->reader_surname);
    }
    free(records);
    fclose(file);
}
void display_experiment_file() {
    FILE *file = fopen("experiment.txt", "r");
    char buffer[500];
    while (fgets(buffer, sizeof(buffer), file)) {
        printf("%s", buffer);
    }
    fclose(file);
}
void calculate_publishers_and_sort() {
    FILE *file = fopen("experiment.txt", "r");
    PubCount *pubs = NULL;
    int unique_pubs = 0;
    char buffer[500];
    while (fgets(buffer, sizeof(buffer), file)) {
        char publisher[50];
        // Parse only publisher from the formatted line
        char *p = buffer;
        int sep_count = 0;
        char *pub_start = NULL;
        // Find 3rd field (publisher)
        int current_sep = 0;
        char *token = strtok(buffer, "|");
        while (token != NULL) {
            current_sep++;
            if (current_sep == 3) {
                strcpy(publisher, token);
                break;
            }
            token = strtok(NULL, "|");
        }
        int found = 0;
        for (int i = 0; i < unique_pubs; i++) {
            if (strcmp(pubs[i].publisher, publisher) == 0) {
                pubs[i].count++;
                found = 1; break;
            }
        }
        if (!found) {
            unique_pubs++;
            pubs = (PubCount *)realloc(pubs, unique_pubs * sizeof(PubCount));
            strcpy(pubs[unique_pubs - 1].publisher, publisher);
            pubs[unique_pubs - 1].count = 1;
        }
    }
    fclose(file);
    if (unique_pubs == 0)
        return;
    // Descending Sort
    for (int i = 0; i < unique_pubs - 1; i++) {
        for (int j = i + 1; j < unique_pubs; j++) {
            if (pubs[j].count > pubs[i].count) {
                PubCount temp = pubs[i];
                pubs[i] = pubs[j];
                pubs[j] = temp;
            }
        }
    }
    FILE *output = fopen("output.txt", "w");
    if (output) {
        for (int i = 0; i < unique_pubs; i++) {
            fprintf(output, "%s - %d\n", pubs[i].publisher, pubs[i].count);
        }
        fclose(output);
    }
    free(pubs);
}
void merge_files() {
    FILE *f_exp = fopen("experiment.txt", "r");
    FILE *f_out = fopen("output.txt", "r");
    FILE *f_res = fopen("result.txt", "w");
    if (!f_res) return;
    char buffer[500];
    fprintf(f_res, "====== BIBLIOGRAPHIC REGISTRY ======\n");
    if (f_exp) {
        while (fgets(buffer, sizeof(buffer), f_exp)) fprintf(f_res, "%s", buffer);
        fclose(f_exp);
    }
    fprintf(f_res, "\n====== PUBLISHER STATISTICS ======\n");
    if (f_out) {
        while (fgets(buffer, sizeof(buffer), f_out)) fprintf(f_res, "%s", buffer);
        fclose(f_out);
    }
    fclose(f_res);
}
int main() {
    int num_records;
    printf("=== Task 5.2: UDT & Files ===\n");
    printf("Enter the number of bibliographic records to input: ");
    scanf("%d", &num_records);
    create_and_populate_file(num_records);
    printf("\n--- Data retrieved from experiment.txt ---\n");
    display_experiment_file();
    calculate_publishers_and_sort();
    printf("\nPublisher counts sorted and saved to output.txt\n");
    merge_files();
    printf("Files successfully merged into result.txt\n");
    return 0;
}
