#include <stdio.h>
#include <stdlib.h>
#include <string.h>
void input_and_save_string(char **str) {
    FILE *file = fopen("input.txt", "w");
    char buffer[2048]; // Temporary buffer for keyboard input
    printf("Enter a string (can contain multiple sentences):\n> ");
    // Note: In a real CLI environment, I'll simulate the input or you can type it
    scanf(" %[^\n]", buffer);
    // Dynamic memory allocation using pointers
    *str = (char *)malloc((strlen(buffer) + 1) * sizeof(char));
    strcpy(*str, buffer);
    fprintf(file, "%s", *str);
    fclose(file);
    printf("String saved to input.txt\n");
}
void process_sentences(char *str, int *q_count, int *first_q_len) {
    *q_count = 0;
    *first_q_len = 0;
    int current_sentence_len = 0;
    char *ptr = str; // Pointer arithmetic
    while (*ptr != '\0') { //we check until stop sign in string
        if (current_sentence_len == 0 && *ptr == ' ') {
            ptr++;
            continue;
        }
        current_sentence_len++;
        if (*ptr == '?') {
            if (current_sentence_len > 1) {
                (*q_count)++;
                if (*q_count == 1) {
                    *first_q_len = current_sentence_len;
                }
            }
            current_sentence_len = 0; // Reset for the next sentence
        } else if (*ptr == '.' || *ptr == '!' || *ptr == ';') {
            current_sentence_len = 0; // Reset if it's not a question
        }
        ptr++;
    }
}
void write_results_to_file(int q_count, int first_q_len) {
    FILE *file = fopen("output.txt", "w");
    if (!file) {
        printf("Error\n");
        return;
    }
    fprintf(file, "interrogative sentences: %d\n", q_count);
    if (q_count > 0) {
        fprintf(file, "Length of the first interrogative sentence: %d characters\n", first_q_len);
    } else {
        fprintf(file, "No interrogative sentences found.\n");
    }
    fclose(file);
    printf("[Results written to output.txt\n");
}
int main() {
    char *input_string = NULL;
    int count_questions = 0;
    int length_first_question = 0;
    printf("=== Task 5.1: String Processing ===\n");
    //wECreate file and read string from keyboard
    input_and_save_string(&input_string);
    //look at string to find sentences
    process_sentences(input_string, &count_questions, &length_first_question);
    printf("\n--- Results ---\n");
    printf("Total interrogative sentences: %d\n", count_questions);
    if (count_questions > 0) {
        printf("Length of the first interrogative sentence: %d characters\n", length_first_question);
    } else {
        printf("No interrogative sentences were found in the string.\n");
    }
    // out output
    write_results_to_file(count_questions, length_first_question);
    free(input_string); //we free space
    return 0;
}
